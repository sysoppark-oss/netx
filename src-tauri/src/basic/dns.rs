//! dig (기본 진단). DNS 서버에 직접 묻고 dig와 같은 모양으로 답을 적는다.

use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr};
use std::time::{Duration, Instant};

use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpStream, UdpSocket};

use super::{Emit, clean, unsupported};

const TYPES: [(&str, u16); 9] = [("A", 1), ("NS", 2), ("CNAME", 5), ("SOA", 6), ("PTR", 12), ("MX", 15), ("TXT", 16), ("AAAA", 28), ("SRV", 33)];
const WAIT: Duration = Duration::from_secs(3);
const TRIES: u32 = 2;

#[derive(Debug, PartialEq)]
pub struct Plan {
    name: String,
    qtype: u16,
    short: bool,
    server: Option<IpAddr>,
}

fn type_code(text: &str) -> Option<u16> {
    TYPES.iter().find(|(n, _)| n.eq_ignore_ascii_case(text)).map(|(_, c)| *c)
}

fn type_name(code: u16) -> String {
    TYPES.iter().find(|(_, c)| *c == code).map(|(n, _)| n.to_string()).unwrap_or_else(|| format!("TYPE{code}"))
}

fn check_name(name: &str) -> Result<String, String> {
    let bare = name.trim_end_matches('.');
    let ok = !bare.is_empty() && bare.len() <= 253 && bare.split('.').all(|l| !l.is_empty() && l.len() <= 63 && l.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_'));
    if ok { Ok(bare.to_string()) } else { Err("도메인 이름이 올바르지 않습니다 (한글 도메인은 xn-- 형식으로 적습니다)".into()) }
}

/// 주소를 거꾸로 찾는 이름 (`-x`)
pub fn reverse_name(ip: IpAddr) -> String {
    match ip {
        IpAddr::V4(v4) => {
            let o = v4.octets();
            format!("{}.{}.{}.{}.in-addr.arpa", o[3], o[2], o[1], o[0])
        }
        IpAddr::V6(v6) => {
            let nibbles: Vec<String> = v6.octets().iter().rev().flat_map(|b| [format!("{:x}", b & 15), format!("{:x}", b >> 4)]).collect();
            format!("{}.ip6.arpa", nibbles.join("."))
        }
    }
}

impl Plan {
    pub fn parse(argv: &[String]) -> Result<Plan, String> {
        let (mut name, mut qtype, mut short, mut server) = (None, None, false, None);
        let mut it = argv.iter();
        while let Some(arg) = it.next() {
            match arg.as_str() {
                "--" | "-4" => {}
                "+short" => short = true,
                "-x" => {
                    let ip: IpAddr = it.next().and_then(|v| v.parse().ok()).ok_or("-x 뒤에는 IP 주소를 적습니다")?;
                    name = Some(reverse_name(ip));
                    qtype = Some(12);
                }
                "-t" => qtype = Some(it.next().and_then(|v| type_code(v)).ok_or_else(|| unsupported("그 레코드 종류"))?),
                a if a.starts_with('@') => server = Some(a[1..].parse().map_err(|_| unsupported("이름으로 적은 DNS 서버(IP 주소로 적으세요)"))?),
                a if a.starts_with('+') || a.starts_with('-') => return Err(unsupported(&format!("`{a}` 옵션"))),
                a => match (type_code(a), &name) {
                    // `dig 이름 종류`: 이름이 이미 있으면 종류로 읽는다.
                    (Some(code), Some(_)) => qtype = Some(code),
                    (_, None) => name = Some(check_name(a)?),
                    _ => return Err("이름은 하나만 적습니다".into()),
                },
            }
        }
        Ok(Plan { name: name.ok_or("물어볼 이름이 없습니다")?, qtype: qtype.unwrap_or(1), short, server })
    }
}

/// 질의 패킷
pub fn query(id: u16, name: &str, qtype: u16) -> Vec<u8> {
    let mut p = Vec::with_capacity(32 + name.len());
    p.extend_from_slice(&id.to_be_bytes());
    p.extend_from_slice(&[0x01, 0x00, 0, 1, 0, 0, 0, 0, 0, 0]);
    for label in name.split('.').filter(|l| !l.is_empty()) {
        p.push(label.len().min(63) as u8);
        p.extend_from_slice(&label.as_bytes()[..label.len().min(63)]);
    }
    p.push(0);
    p.extend_from_slice(&qtype.to_be_bytes());
    p.extend_from_slice(&[0, 1]);
    p
}

fn u16_at(p: &[u8], at: usize) -> Option<u16> {
    Some(u16::from_be_bytes([*p.get(at)?, *p.get(at + 1)?]))
}

/// 패킷 안의 이름을 읽는다(압축 포함). (이름, 이름 다음 위치)
fn read_name(p: &[u8], mut at: usize) -> Option<(String, usize)> {
    let mut name = String::new();
    let mut next = None;
    for _ in 0..64 {
        let len = *p.get(at)? as usize;
        if len == 0 {
            return Some((if name.is_empty() { ".".into() } else { name }, next.unwrap_or(at + 1)));
        }
        if len & 0xc0 == 0xc0 {
            let to = (len & 0x3f) << 8 | *p.get(at + 1)? as usize;
            next.get_or_insert(at + 2);
            at = to;
            continue;
        }
        if len > 63 || name.len() > 255 {
            return None;
        }
        let label = p.get(at + 1..at + 1 + len)?;
        name.extend(label.iter().map(|b| if b.is_ascii_alphanumeric() || b"-_".contains(b) { *b as char } else { '?' }));
        name.push('.');
        at += 1 + len;
    }
    None
}

#[derive(Debug, PartialEq)]
pub struct Record {
    name: String,
    ttl: u32,
    rtype: u16,
    data: String,
}

#[derive(Debug, PartialEq)]
pub struct Answer {
    rcode: u8,
    truncated: bool,
    records: Vec<Record>,
}

fn rdata(p: &[u8], rtype: u16, at: usize, len: usize) -> Option<String> {
    let raw = p.get(at..at + len)?;
    Some(match rtype {
        1 if len == 4 => Ipv4Addr::new(raw[0], raw[1], raw[2], raw[3]).to_string(),
        28 if len == 16 => Ipv6Addr::from(<[u8; 16]>::try_from(raw).ok()?).to_string(),
        2 | 5 | 12 => read_name(p, at)?.0,
        15 => format!("{} {}", u16_at(p, at)?, read_name(p, at + 2)?.0),
        33 => format!("{} {} {} {}", u16_at(p, at)?, u16_at(p, at + 2)?, u16_at(p, at + 4)?, read_name(p, at + 6)?.0),
        6 => {
            let (mname, next) = read_name(p, at)?;
            let (rname, next) = read_name(p, next)?;
            let nums: Vec<String> = (0..5).filter_map(|i| p.get(next + i * 4..next + i * 4 + 4).map(|b| u32::from_be_bytes([b[0], b[1], b[2], b[3]]).to_string())).collect();
            format!("{mname} {rname} {}", nums.join(" "))
        }
        16 => {
            let mut parts = Vec::new();
            let mut i = 0;
            while i < raw.len() {
                let n = raw[i] as usize;
                let text = raw.get(i + 1..i + 1 + n)?;
                parts.push(format!("\"{}\"", text.iter().map(|b| if (0x20..0x7f).contains(b) && *b != b'"' { *b as char } else { '?' }).collect::<String>()));
                i += 1 + n;
            }
            parts.join(" ")
        }
        _ => format!("\\# {len}"),
    })
}

/// 답 패킷을 읽는다. 질의 번호가 다르거나 깨진 패킷이면 None.
pub fn parse(p: &[u8], id: u16) -> Option<Answer> {
    if u16_at(p, 0)? != id || p.get(2)? & 0x80 == 0 {
        return None;
    }
    let truncated = p[2] & 0x02 != 0;
    let rcode = p.get(3)? & 0x0f;
    let (questions, answers) = (u16_at(p, 4)?, u16_at(p, 6)?);
    let mut at = 12;
    for _ in 0..questions {
        at = read_name(p, at)?.1 + 4;
    }
    let mut records = Vec::new();
    for _ in 0..answers.min(200) {
        let Some((name, next)) = read_name(p, at) else { break };
        let (Some(rtype), Some(len)) = (u16_at(p, next), u16_at(p, next + 8)) else { break };
        let ttl = p.get(next + 4..next + 8).map(|b| u32::from_be_bytes([b[0], b[1], b[2], b[3]])).unwrap_or(0);
        let Some(data) = rdata(p, rtype, next + 10, len as usize) else { break };
        records.push(Record { name, ttl, rtype, data });
        at = next + 10 + len as usize;
    }
    Some(Answer { rcode, truncated, records })
}

/// 답 패킷에 든 PTR 레코드의 이름들(mDNS 서비스 찾기에 쓴다). 번호는 따지지 않는다.
pub fn ptr_names(p: &[u8]) -> Vec<String> {
    let Some(id) = u16_at(p, 0) else { return Vec::new() };
    parse(p, id).map(|a| a.records.into_iter().filter(|r| r.rtype == 12).map(|r| r.data).collect()).unwrap_or_default()
}

fn rcode_name(code: u8) -> String {
    match code {
        0 => "NOERROR".into(),
        1 => "FORMERR".into(),
        2 => "SERVFAIL".into(),
        3 => "NXDOMAIN".into(),
        5 => "REFUSED".into(),
        n => format!("RCODE{n}"),
    }
}

async fn ask_udp(server: SocketAddr, packet: &[u8], id: u16) -> Option<Answer> {
    let sock = UdpSocket::bind(if server.is_ipv4() { "0.0.0.0:0" } else { "[::]:0" }).await.ok()?;
    sock.connect(server).await.ok()?;
    let mut buf = [0u8; 4096];
    for _ in 0..TRIES {
        sock.send(packet).await.ok()?;
        let deadline = Instant::now() + WAIT;
        // 번호가 다른 답은 버리고 계속 기다린다.
        while let Ok(Ok(n)) = tokio::time::timeout(deadline.saturating_duration_since(Instant::now()), sock.recv(&mut buf)).await {
            if let Some(answer) = parse(&buf[..n], id) {
                return Some(answer);
            }
        }
    }
    None
}

async fn ask_tcp(server: SocketAddr, packet: &[u8], id: u16) -> Option<Answer> {
    let work = async {
        let mut stream = TcpStream::connect(server).await.ok()?;
        stream.write_all(&(packet.len() as u16).to_be_bytes()).await.ok()?;
        stream.write_all(packet).await.ok()?;
        let len = stream.read_u16().await.ok()? as usize;
        let mut buf = vec![0u8; len];
        stream.read_exact(&mut buf).await.ok()?;
        parse(&buf, id)
    };
    tokio::time::timeout(WAIT * 2, work).await.ok().flatten()
}

async fn system_server() -> Option<IpAddr> {
    crate::netinfo::settled().await.dns.iter().find_map(|s| s.parse().ok())
}

pub async fn run(plan: Plan, emit: Emit<'_>) -> Result<i32, String> {
    let server = match plan.server {
        Some(s) => s,
        None => system_server().await.ok_or("이 PC의 DNS 서버 주소를 찾지 못했습니다. `@8.8.8.8`처럼 서버를 적어 주세요.")?,
    };
    run_with(plan, SocketAddr::new(server, 53), emit).await
}

async fn run_with(plan: Plan, server: SocketAddr, emit: Emit<'_>) -> Result<i32, String> {
    let id = (std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.subsec_nanos()).unwrap_or(0) >> 4) as u16;
    let packet = query(id, &plan.name, plan.qtype);
    let started = Instant::now();
    let mut answer = ask_udp(server, &packet, id).await;
    let mut transport = "UDP";
    if answer.as_ref().is_some_and(|a| a.truncated) {
        answer = ask_tcp(server, &packet, id).await;
        transport = "TCP";
    }
    let Some(answer) = answer else {
        emit(format!(";; communications error to {}#{}: timed out", server.ip(), server.port()));
        emit(";; no servers could be reached".into());
        return Ok(9);
    };
    if plan.short {
        for r in &answer.records {
            emit(clean(&r.data));
        }
        return Ok(0);
    }
    emit(format!(";; ->>HEADER<<- opcode: QUERY, status: {}, id: {id}", rcode_name(answer.rcode)));
    emit(format!(";; QUERY: 1, ANSWER: {}", answer.records.len()));
    emit(String::new());
    emit(";; QUESTION SECTION:".into());
    emit(format!(";{}.\t\tIN\t{}", plan.name, type_name(plan.qtype)));
    if !answer.records.is_empty() {
        emit(String::new());
        emit(";; ANSWER SECTION:".into());
        for r in &answer.records {
            emit(clean(&format!("{}\t{}\tIN\t{}\t{}", r.name, r.ttl, type_name(r.rtype), r.data)));
        }
    }
    emit(String::new());
    emit(format!(";; Query time: {} msec", started.elapsed().as_millis()));
    emit(format!(";; SERVER: {}#{}({}) ({transport})", server.ip(), server.port(), server.ip()));
    Ok(0)
}

#[cfg(test)]
mod tests {
    use super::super::ping::tests::{collect, s};
    use super::*;

    /// 시험용 답 패킷: example.com → CNAME → A 두 개, 뒤쪽 이름은 압축
    fn reply(id: u16, rcode: u8, truncated: bool) -> Vec<u8> {
        let mut p = query(id, "example.com", 1);
        p[2] = 0x81 | if truncated { 0x02 } else { 0 };
        p[3] = 0x80 | rcode;
        if rcode == 0 && !truncated {
            p[7] = 3;
            // CNAME: example.com → www.example.com (이름 압축: 0xc00c = 질문의 이름)
            p.extend_from_slice(&[0xc0, 0x0c, 0, 5, 0, 1, 0, 0, 1, 44, 0, 6, 3, b'w', b'w', b'w', 0xc0, 0x0c]);
            let cname_at = p.len() - 6;
            for last in [34u8, 35] {
                p.extend_from_slice(&[0xc0, cname_at as u8, 0, 1, 0, 1, 0, 0, 0, 60, 0, 4, 93, 184, 216, last]);
            }
        }
        p
    }

    #[test]
    fn options() {
        assert_eq!(Plan::parse(&s(&["+short", "example.com"])).unwrap(), Plan { name: "example.com".into(), qtype: 1, short: true, server: None });
        let p = Plan::parse(&s(&["@8.8.8.8", "example.com.", "mx"])).unwrap();
        assert_eq!((p.qtype, p.server, p.name.as_str()), (15, Some("8.8.8.8".parse().unwrap()), "example.com"));
        let p = Plan::parse(&s(&["-x", "192.168.0.7"])).unwrap();
        assert_eq!((p.name.as_str(), p.qtype), ("7.0.168.192.in-addr.arpa", 12));
        assert!(reverse_name("2001:db8::1".parse().unwrap()).starts_with("1.0.0.0.") && reverse_name("2001:db8::1".parse().unwrap()).ends_with("8.b.d.0.1.0.0.2.ip6.arpa"));
        // 이름이 종류 이름과 같아도(예: "a") 첫 낱말은 이름이다.
        assert_eq!(Plan::parse(&s(&["a"])).unwrap().name, "a");
        for bad in [vec!["+trace", "example.com"], vec!["@dns.google", "example.com"], vec!["exa mple.com"], vec!["a.com", "b.com"], vec!["-t", "ZZ", "a.com"], vec![], vec!["한글.kr"]] {
            assert!(Plan::parse(&s(&bad)).is_err(), "{bad:?}");
        }
    }

    #[test]
    fn reads_answers() {
        let a = parse(&reply(7, 0, false), 7).unwrap();
        assert_eq!(a.rcode, 0);
        assert_eq!(a.records.iter().map(|r| (r.rtype, r.data.as_str(), r.ttl)).collect::<Vec<_>>(), [(5, "www.example.com.", 300), (1, "93.184.216.34", 60), (1, "93.184.216.35", 60)]);
        assert_eq!(a.records[1].name, "www.example.com.");
        assert_eq!(parse(&reply(7, 3, false), 7).unwrap().rcode, 3);
        // 번호가 다른 답, 질의 패킷(답이 아님)은 버린다.
        assert!(parse(&reply(7, 0, false), 8).is_none());
        assert!(parse(&query(7, "example.com", 1), 7).is_none());
    }

    #[test]
    fn broken_packets_never_panic() {
        let good = reply(7, 0, false);
        for cut in 0..good.len() {
            let _ = parse(&good[..cut], 7);
        }
        // 자기 자신을 가리키는 압축 이름(끝없는 고리)
        let mut looped = reply(7, 0, false);
        let at = looped.len() - 16;
        looped[at] = 0xc0;
        looped[at + 1] = at as u8;
        let _ = parse(&looped, 7);
        let mut seed = 0x9e37_79b9u32;
        for _ in 0..3000 {
            let mut p = good.clone();
            for _ in 0..4 {
                seed ^= seed << 13;
                seed ^= seed >> 17;
                seed ^= seed << 5;
                let i = 2 + seed as usize % (p.len() - 2);
                p[i] = (seed >> 8) as u8;
            }
            let _ = parse(&p, 7);
        }
        // TXT와 그 밖의 종류
        assert_eq!(rdata(&[3, b'a', b'"', 7, 1, b'b'], 16, 0, 6).unwrap(), "\"a??\" \"b\"");
        assert_eq!(rdata(&[1, 2, 3], 99, 0, 3).unwrap(), "\\# 3");
    }

    /// 가짜 DNS 서버(UDP)로 처음부터 끝까지
    #[tokio::test]
    async fn asks_a_server() {
        let sock = UdpSocket::bind("127.0.0.1:0").await.unwrap();
        let server = sock.local_addr().unwrap();
        tokio::spawn(async move {
            let mut buf = [0u8; 512];
            loop {
                let (n, from) = sock.recv_from(&mut buf).await.unwrap();
                let id = u16::from_be_bytes([buf[0], buf[1]]);
                let nx = buf[..n].windows(4).any(|w| w == b"nope");
                // 엉뚱한 번호의 답을 먼저 보낸다(버려져야 한다).
                sock.send_to(&reply(id.wrapping_add(1), 3, false), from).await.unwrap();
                sock.send_to(&reply(id, if nx { 3 } else { 0 }, false), from).await.unwrap();
            }
        });
        let (lines, emit) = collect();
        let code = run_with(Plan::parse(&s(&["+short", "example.com"])).unwrap(), server, &emit).await.unwrap();
        let short = lines.lock().unwrap().clone();
        assert_eq!((code, short.as_slice()), (0, &["www.example.com.".to_string(), "93.184.216.34".to_string(), "93.184.216.35".to_string()][..]));
        assert_eq!(crate::interpret::interpret("dig", &short, crate::engine::StepStatus::Done, Some(0)).light, crate::interpret::Light::Green);

        let (lines, emit) = collect();
        run_with(Plan::parse(&s(&["nope.example"])).unwrap(), server, &emit).await.unwrap();
        let long = lines.lock().unwrap().clone();
        assert!(long[0].contains("status: NXDOMAIN"), "{long:?}");
        assert!(crate::interpret::interpret("dig", &long, crate::engine::StepStatus::Done, Some(0)).text.contains("NXDOMAIN"));
    }

    #[tokio::test]
    async fn silent_server() {
        let sock = UdpSocket::bind("127.0.0.1:0").await.unwrap();
        let (lines, emit) = collect();
        let code = run_with(Plan::parse(&s(&["example.com"])).unwrap(), sock.local_addr().unwrap(), &emit).await.unwrap();
        assert_eq!(code, 9);
        assert!(crate::interpret::interpret("dig", &lines.lock().unwrap(), crate::engine::StepStatus::Done, Some(9)).text.contains("응답하지 않습니다"));
    }
}
