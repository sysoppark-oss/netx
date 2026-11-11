//! 캡처 파일 분석: 저장해 둔 pcap을 열어 요약, 문제 찾기, 패킷 목록, 패킷 상세를 만든다.
//!
//! 읽기 전용이다. 작업 폴더의 캡처 파일만 이름으로 열고, 아무것도 밖으로 보내지 않는다.
//! 캡처 파일은 밖에서 온 자료이므로 색인 접근 없이 `get`으로만 읽고, 글자는 제어 문자를 지운 뒤 쓴다.
//! 옛 형식 pcap만 읽는다(pcapng는 읽지 않는다). 깨진 파일은 읽은 데까지만 쓴다.

use std::collections::hash_map::DefaultHasher;
use std::collections::{HashMap, HashSet};
use std::hash::{Hash, Hasher};
use std::net::{Ipv4Addr, Ipv6Addr};
use std::path::Path;
use std::sync::Mutex;
use std::time::SystemTime;

use serde::Serialize;

const MAX_FILE: u64 = 64 * 1024 * 1024;
const MAX_PACKETS: usize = 200_000;
/// 한 번에 돌려주는 줄 수의 상한
const PAGE_MAX: usize = 500;
/// 16진수 보기로 보여 주는 바이트 수의 상한
const HEX_MAX: usize = 2048;

// ------------------------------------------------------------------ 화면으로 보내는 자료

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct CaptureFile {
    pub name: String,
    pub size: u64,
    /// 고친 시각 (1970년부터의 초)
    pub modified: u64,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct Share {
    pub label: String,
    pub packets: u64,
    pub bytes: u64,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct Talk {
    pub a: String,
    pub b: String,
    pub packets: u64,
    pub bytes: u64,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct Finding {
    /// red, yellow
    pub light: &'static str,
    pub title: String,
    pub text: String,
    pub count: u64,
    /// 패킷 목록을 이 문제로 거를 때 쓰는 표시
    pub mark: &'static str,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct Analysis {
    pub name: String,
    pub size: u64,
    /// 첫 패킷의 시각 (1970년부터의 초)
    pub started: f64,
    pub seconds: f64,
    pub packets: u64,
    pub bytes: u64,
    pub link: String,
    pub protocols: Vec<Share>,
    pub talks: Vec<Talk>,
    pub findings: Vec<Finding>,
    pub notes: Vec<String>,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct Row {
    pub no: u32,
    /// 첫 패킷부터의 초
    pub time: f64,
    pub src: String,
    pub dst: String,
    pub proto: String,
    pub len: u32,
    pub info: String,
    /// 문제 표시. 없으면 빈 글자.
    pub mark: &'static str,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct Page {
    pub total: usize,
    pub rows: Vec<Row>,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct Field {
    pub name: String,
    pub value: String,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct Layer {
    pub name: String,
    pub fields: Vec<Field>,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct Detail {
    pub no: u32,
    pub time: f64,
    /// 실제 길이와 파일에 담긴 길이
    pub len: u32,
    pub captured: u32,
    pub layers: Vec<Layer>,
    pub hex: Vec<String>,
    /// 16진수 보기가 상한에서 잘렸는지
    pub cut: bool,
}

// ------------------------------------------------------------------ 바이트 읽기

fn b8(b: &[u8], at: usize) -> Option<u8> {
    b.get(at).copied()
}

fn be16(b: &[u8], at: usize) -> Option<u16> {
    Some(u16::from_be_bytes([*b.get(at)?, *b.get(at + 1)?]))
}

fn be32(b: &[u8], at: usize) -> Option<u32> {
    Some(u32::from_be_bytes([*b.get(at)?, *b.get(at + 1)?, *b.get(at + 2)?, *b.get(at + 3)?]))
}

/// 밖에서 온 글자: 제어 문자와 글자가 아닌 바이트를 점으로 바꾸고 길이를 줄인다.
fn clean(bytes: &[u8], max: usize) -> String {
    bytes.iter().take(max).map(|&c| if (0x20..0x7f).contains(&c) { c as char } else { '.' }).collect()
}

fn mac(b: &[u8], at: usize) -> Option<String> {
    let m = b.get(at..at + 6)?;
    Some(m.iter().map(|x| format!("{x:02x}")).collect::<Vec<_>>().join(":"))
}

fn ip4(b: &[u8], at: usize) -> Option<String> {
    let a = b.get(at..at + 4)?;
    Some(Ipv4Addr::new(*a.first()?, *a.get(1)?, *a.get(2)?, *a.get(3)?).to_string())
}

fn ip6(b: &[u8], at: usize) -> Option<String> {
    let a: [u8; 16] = b.get(at..at + 16)?.try_into().ok()?;
    Some(Ipv6Addr::from(a).to_string())
}

fn hash_of<T: Hash>(v: &T) -> u64 {
    let mut h = DefaultHasher::new();
    v.hash(&mut h);
    h.finish()
}

// ------------------------------------------------------------------ 패킷 한 개 해석

/// 상세 보기일 때만 계층별 값을 모은다. 목록을 만들 때는 아무것도 모으지 않는다.
struct Out {
    detail: bool,
    layers: Vec<Layer>,
}

impl Out {
    fn layer(&mut self, name: &str) {
        if self.detail {
            self.layers.push(Layer { name: name.to_string(), fields: Vec::new() });
        }
    }

    fn put(&mut self, name: &str, value: impl FnOnce() -> String) {
        if self.detail {
            if let Some(l) = self.layers.last_mut() {
                l.fields.push(Field { name: name.to_string(), value: value() });
            }
        }
    }
}

#[derive(Debug, Default, Clone, Copy, PartialEq)]
struct Tcp {
    seq: u32,
    len: usize,
    flags: u8,
    window: u16,
}

#[derive(Debug, Default, Clone, PartialEq)]
struct Dns {
    id: u16,
    response: bool,
    rcode: u8,
    name: String,
}

#[derive(Debug, Default)]
struct Seen {
    src: String,
    dst: String,
    sport: u16,
    dport: u16,
    proto: String,
    info: String,
    /// IP 패킷인지 (대화 상대 집계에 쓴다)
    ip: bool,
    tcp: Option<Tcp>,
    dns: Option<Dns>,
    unreachable: bool,
}

fn ether_name(ty: u16) -> &'static str {
    match ty {
        0x88cc => "LLDP",
        0x8863 | 0x8864 => "PPPoE",
        0x8847 | 0x8848 => "MPLS",
        0x888e => "EAPOL",
        0x8035 => "RARP",
        0x88a8 => "QinQ",
        _ => "이더넷",
    }
}

fn decode(link: u32, frame: &[u8], out: &mut Out) -> Seen {
    let mut seen = Seen { proto: "?".into(), ..Seen::default() };
    let l3: Option<(u16, &[u8])> = match link {
        1 => ethernet(frame, out, &mut seen),
        113 => {
            out.layer("리눅스 캡처 헤더 (SLL)");
            be16(frame, 14).zip(frame.get(16..))
        }
        276 => {
            out.layer("리눅스 캡처 헤더 (SLL2)");
            out.put("인터페이스 번호", || be32(frame, 4).map(|v| v.to_string()).unwrap_or_default());
            be16(frame, 0).zip(frame.get(20..))
        }
        12 | 101 | 228 | 229 => frame.first().map(|v| (if v >> 4 == 6 { 0x86dd } else { 0x0800 }, frame)),
        _ => None,
    };
    let done = match l3 {
        Some((0x0800, ip)) => ipv4(ip, out, &mut seen),
        Some((0x86dd, ip)) => ipv6(ip, out, &mut seen),
        Some((0x0806, a)) => arp(a, out, &mut seen),
        Some((ty, _)) => {
            if seen.proto == "?" {
                seen.proto = ether_name(ty).to_string();
            }
            if seen.info.is_empty() {
                seen.info = format!("EtherType 0x{ty:04x}");
            }
            Some(())
        }
        None => None,
    };
    if done.is_none() && seen.info.is_empty() {
        seen.info = "해석할 수 없는 패킷 (잘렸거나 모르는 형식)".into();
    }
    seen
}

fn ethernet<'a>(frame: &'a [u8], out: &mut Out, seen: &mut Seen) -> Option<(u16, &'a [u8])> {
    out.layer("이더넷");
    let (dst, src) = (mac(frame, 0)?, mac(frame, 6)?);
    out.put("받는 MAC", || dst.clone());
    out.put("보낸 MAC", || src.clone());
    let stp = dst == "01:80:c2:00:00:00";
    seen.dst = dst;
    seen.src = src;
    let mut ty = be16(frame, 12)?;
    let mut at = 14;
    if ty == 0x8100 {
        let tag = be16(frame, 14)?;
        out.put("VLAN", || (tag & 0x0fff).to_string());
        ty = be16(frame, 16)?;
        at = 18;
    }
    out.put("종류", || format!("0x{ty:04x}"));
    if ty < 0x0600 {
        // 802.3 길이 칸: 스패닝 트리 같은 LLC 프레임
        seen.proto = if stp { "STP" } else { "LLC" }.to_string();
        seen.info = if stp { "스패닝 트리 알림(BPDU)".into() } else { "LLC 프레임".into() };
    }
    Some((ty, frame.get(at..)?))
}

fn arp(a: &[u8], out: &mut Out, seen: &mut Seen) -> Option<()> {
    out.layer("ARP");
    seen.proto = "ARP".into();
    let op = be16(a, 6)?;
    let (sender_mac, sender_ip, target_ip) = (mac(a, 8)?, ip4(a, 14)?, ip4(a, 24)?);
    out.put("동작", || match op {
        1 => "요청".into(),
        2 => "응답".into(),
        n => n.to_string(),
    });
    out.put("보낸 쪽", || format!("{sender_ip} ({sender_mac})"));
    out.put("찾는 주소", || target_ip.clone());
    seen.info = match op {
        1 => format!("{target_ip}은(는) 누구입니까? {sender_ip}에게 알려 주세요"),
        2 => format!("{sender_ip}은(는) {sender_mac}입니다"),
        n => format!("ARP 동작 {n}"),
    };
    Some(())
}

fn ipv4(ip: &[u8], out: &mut Out, seen: &mut Seen) -> Option<()> {
    out.layer("IPv4");
    seen.proto = "IPv4".into();
    let header = usize::from(b8(ip, 0)? & 0x0f) * 4;
    let total = usize::from(be16(ip, 2)?);
    let (ttl, next) = (b8(ip, 8)?, b8(ip, 9)?);
    let frag = be16(ip, 6)?;
    let (src, dst) = (ip4(ip, 12)?, ip4(ip, 16)?);
    out.put("보낸 주소", || src.clone());
    out.put("받는 주소", || dst.clone());
    out.put("TTL", || ttl.to_string());
    out.put("전체 길이", || total.to_string());
    out.put("상위 프로토콜", || format!("{next} ({})", ip_proto_name(next)));
    seen.src = src;
    seen.dst = dst;
    seen.ip = true;
    if header < 20 {
        return None;
    }
    if frag & 0x1fff != 0 {
        out.put("조각 위치", || (u32::from(frag & 0x1fff) * 8).to_string());
        seen.info = format!("IP 조각 (위치 {}, {})", u32::from(frag & 0x1fff) * 8, ip_proto_name(next));
        return Some(());
    }
    if frag & 0x2000 != 0 {
        out.put("조각", || "첫 조각 (뒤에 더 있음)".into());
    }
    transport(next, ip.get(header..)?, total.saturating_sub(header), false, out, seen)
}

fn ipv6(ip: &[u8], out: &mut Out, seen: &mut Seen) -> Option<()> {
    out.layer("IPv6");
    seen.proto = "IPv6".into();
    let payload_len = usize::from(be16(ip, 4)?);
    let (mut next, hop) = (b8(ip, 6)?, b8(ip, 7)?);
    let (src, dst) = (ip6(ip, 8)?, ip6(ip, 24)?);
    out.put("보낸 주소", || src.clone());
    out.put("받는 주소", || dst.clone());
    out.put("홉 제한", || hop.to_string());
    seen.src = src;
    seen.dst = dst;
    seen.ip = true;
    let mut rest = ip.get(40..)?;
    let mut left = payload_len;
    // 확장 헤더를 몇 겹까지만 벗긴다.
    for _ in 0..6 {
        let skip = match next {
            0 | 43 | 60 => (usize::from(b8(rest, 1)?) + 1) * 8,
            44 => 8,
            _ => break,
        };
        next = b8(rest, 0)?;
        rest = rest.get(skip..)?;
        left = left.saturating_sub(skip);
    }
    out.put("상위 프로토콜", || format!("{next} ({})", ip_proto_name(next)));
    transport(next, rest, left, true, out, seen)
}

fn ip_proto_name(n: u8) -> &'static str {
    match n {
        1 => "ICMP",
        2 => "IGMP",
        4 => "IP-in-IP",
        6 => "TCP",
        17 => "UDP",
        41 => "IPv6 터널",
        47 => "GRE",
        50 => "ESP",
        51 => "AH",
        58 => "ICMPv6",
        88 => "EIGRP",
        89 => "OSPF",
        103 => "PIM",
        112 => "VRRP",
        115 => "L2TP",
        132 => "SCTP",
        _ => "기타",
    }
}

fn tcp_flags(flags: u8) -> String {
    let names = [(0x02, "SYN"), (0x01, "FIN"), (0x04, "RST"), (0x08, "PSH"), (0x10, "ACK"), (0x20, "URG")];
    names.iter().filter(|(bit, _)| flags & bit != 0).map(|(_, n)| *n).collect::<Vec<_>>().join(", ")
}

/// `wire_len`은 IP 헤더가 말하는 이 구간의 실제 길이. 캡처가 잘려 있어도 데이터 길이를 바르게 센다.
fn transport(next: u8, p: &[u8], wire_len: usize, v6: bool, out: &mut Out, seen: &mut Seen) -> Option<()> {
    match next {
        6 => {
            out.layer("TCP");
            seen.proto = "TCP".into();
            let (sport, dport) = (be16(p, 0)?, be16(p, 2)?);
            seen.sport = sport;
            seen.dport = dport;
            let (seq, ack) = (be32(p, 4)?, be32(p, 8)?);
            let header = usize::from(b8(p, 12)? >> 4) * 4;
            let (flags, window) = (b8(p, 13)?, be16(p, 14)?);
            let len = wire_len.saturating_sub(header);
            out.put("보낸 포트", || sport.to_string());
            out.put("받는 포트", || dport.to_string());
            out.put("순서 번호(Seq)", || seq.to_string());
            out.put("확인 번호(Ack)", || ack.to_string());
            out.put("플래그", || tcp_flags(flags));
            out.put("창 크기", || window.to_string());
            out.put("데이터 길이", || len.to_string());
            seen.tcp = Some(Tcp { seq, len, flags, window });
            seen.info = format!("{sport} → {dport} [{}] Seq={seq} Ack={ack} Win={window} Len={len}", tcp_flags(flags));
            if let Some(data) = p.get(header.max(20)..).filter(|d| !d.is_empty()) {
                stream_payload(data, out, seen);
            }
            Some(())
        }
        17 => {
            out.layer("UDP");
            let (sport, dport) = (be16(p, 0)?, be16(p, 2)?);
            seen.sport = sport;
            seen.dport = dport;
            let len = usize::from(be16(p, 4)?).saturating_sub(8);
            out.put("보낸 포트", || sport.to_string());
            out.put("받는 포트", || dport.to_string());
            out.put("데이터 길이", || len.to_string());
            seen.proto = udp_label(sport, dport).to_string();
            seen.info = format!("{sport} → {dport} Len={len}");
            if let Some(data) = p.get(8..) {
                datagram_payload(data, out, seen);
            }
            Some(())
        }
        1 if !v6 => icmp4(p, out, seen),
        58 => icmp6(p, out, seen),
        n => {
            seen.proto = match ip_proto_name(n) {
                "기타" => format!("IP 프로토콜 {n}"),
                name => name.to_string(),
            };
            seen.info = format!("{} 패킷 ({wire_len}바이트)", seen.proto);
            Some(())
        }
    }
}

fn icmp4(p: &[u8], out: &mut Out, seen: &mut Seen) -> Option<()> {
    out.layer("ICMP");
    seen.proto = "ICMP".into();
    let (ty, code) = (b8(p, 0)?, b8(p, 1)?);
    out.put("종류", || ty.to_string());
    out.put("코드", || code.to_string());
    seen.info = match ty {
        8 | 0 => {
            let (id, seq) = (be16(p, 4)?, be16(p, 6)?);
            out.put("순번", || seq.to_string());
            format!("핑 {} id={id} seq={seq}", if ty == 8 { "요청" } else { "응답" })
        }
        3 => {
            seen.unreachable = true;
            format!(
                "도달 불가: {}",
                match code {
                    0 => "네트워크에 갈 수 없음".to_string(),
                    1 => "장비에 갈 수 없음".to_string(),
                    2 => "프로토콜을 받지 않음".to_string(),
                    3 => "포트가 닫혀 있음".to_string(),
                    4 => format!("조각내야 하는데 금지됨 (다음 구간 MTU {})", be16(p, 6).unwrap_or(0)),
                    9 | 10 | 13 => "관리자가 막음(방화벽)".to_string(),
                    n => format!("코드 {n}"),
                }
            )
        }
        11 => {
            seen.unreachable = true;
            "시간 초과 (TTL이 다 됨)".into()
        }
        5 => "경로 바꿈 안내(Redirect)".into(),
        n => format!("ICMP 종류 {n} 코드 {code}"),
    };
    Some(())
}

fn icmp6(p: &[u8], out: &mut Out, seen: &mut Seen) -> Option<()> {
    out.layer("ICMPv6");
    seen.proto = "ICMPv6".into();
    let (ty, code) = (b8(p, 0)?, b8(p, 1)?);
    out.put("종류", || ty.to_string());
    out.put("코드", || code.to_string());
    seen.info = match ty {
        128 | 129 => format!("핑 {} seq={}", if ty == 128 { "요청" } else { "응답" }, be16(p, 6)?),
        1 => {
            seen.unreachable = true;
            format!("도달 불가 (코드 {code})")
        }
        2 => "패킷이 너무 큼 (MTU 문제)".into(),
        3 => {
            seen.unreachable = true;
            "시간 초과 (홉 제한이 다 됨)".into()
        }
        133 => "라우터 찾기".into(),
        134 => "라우터 알림".into(),
        135 => format!("이웃 찾기: {}", ip6(p, 8).unwrap_or_default()),
        136 => format!("이웃 알림: {}", ip6(p, 8).unwrap_or_default()),
        130..=132 | 143 => "멀티캐스트 가입 알림(MLD)".into(),
        n => format!("ICMPv6 종류 {n}"),
    };
    Some(())
}

fn udp_label(sport: u16, dport: u16) -> &'static str {
    let name = |p: u16| match p {
        53 => "DNS",
        5353 => "mDNS",
        5355 => "LLMNR",
        67 | 68 => "DHCP",
        546 | 547 => "DHCPv6",
        69 => "TFTP",
        123 => "NTP",
        137 => "NBNS",
        138 => "NetBIOS",
        161 | 162 => "SNMP",
        443 => "QUIC",
        500 | 4500 => "IKE",
        514 => "Syslog",
        520 => "RIP",
        1812 | 1813 => "RADIUS",
        1900 => "SSDP",
        1985 => "HSRP",
        3702 => "WS-Discovery",
        4789 => "VXLAN",
        _ => "",
    };
    match (name(dport), name(sport)) {
        ("", "") => "UDP",
        ("", n) | (n, _) => n,
    }
}

fn datagram_payload(data: &[u8], out: &mut Out, seen: &mut Seen) {
    match seen.proto.as_str() {
        "DNS" | "mDNS" | "LLMNR" => {
            if let Some(d) = dns(data, out) {
                seen.info = dns_info(&d.0, &d.1, d.2);
                if seen.proto == "DNS" {
                    seen.dns = Some(d.0);
                }
            }
        }
        "DHCP" => {
            if let Some(kind) = dhcp_kind(data) {
                out.layer("DHCP");
                out.put("메시지", || kind.to_string());
                seen.info = format!("DHCP {kind}");
            }
        }
        "SSDP" => {
            if let Some(line) = first_line(data) {
                seen.info = line;
            }
        }
        _ => {}
    }
}

fn first_line(data: &[u8]) -> Option<String> {
    let end = data.iter().position(|&c| c == b'\r' || c == b'\n').unwrap_or(data.len());
    let line = data.get(..end)?;
    (!line.is_empty()).then(|| clean(line, 120))
}

/// TCP 데이터의 첫머리로 알아볼 수 있는 것만 알아본다: TLS, HTTP, SSH.
fn stream_payload(data: &[u8], out: &mut Out, seen: &mut Seen) {
    let ports = format!("{} → {}", seen.sport, seen.dport);
    if let (Some(kind), Some(3), Some(minor)) = (b8(data, 0), b8(data, 1), b8(data, 2)) {
        if (0x14..=0x17).contains(&kind) && minor <= 4 {
            out.layer("TLS");
            seen.proto = "TLS".into();
            let what = match (kind, b8(data, 5)) {
                (0x16, Some(1)) => match tls_server_name(data) {
                    Some(name) => {
                        out.put("접속 이름(SNI)", || name.clone());
                        format!("접속 시작(ClientHello) 이름={name}")
                    }
                    None => "접속 시작(ClientHello)".to_string(),
                },
                (0x16, Some(2)) => "서버 응답(ServerHello)".to_string(),
                (0x16, _) => "핸드셰이크".to_string(),
                (0x15, _) => "경고(Alert)".to_string(),
                (0x14, _) => "암호 전환(ChangeCipherSpec)".to_string(),
                _ => "암호화된 데이터".to_string(),
            };
            out.put("내용", || what.clone());
            seen.info = format!("{ports} TLS {what}");
            return;
        }
    }
    const HTTP: [&[u8]; 9] = [b"GET ", b"POST ", b"HEAD ", b"PUT ", b"DELETE ", b"OPTIONS ", b"PATCH ", b"CONNECT ", b"HTTP/1."];
    if HTTP.iter().any(|m| data.starts_with(m)) {
        if let Some(line) = first_line(data) {
            out.layer("HTTP");
            out.put("첫 줄", || line.clone());
            seen.proto = "HTTP".into();
            seen.info = format!("{ports} {line}");
        }
    } else if data.starts_with(b"SSH-") {
        if let Some(line) = first_line(data) {
            out.layer("SSH");
            out.put("버전 알림", || line.clone());
            seen.proto = "SSH".into();
            seen.info = format!("{ports} {line}");
        }
    }
}

/// ClientHello에서 접속하려는 서버 이름(SNI)을 찾는다.
fn tls_server_name(data: &[u8]) -> Option<String> {
    // 레코드 머리 5, 핸드셰이크 머리 4, 버전 2, 난수 32
    let mut at = 43;
    at += 1 + usize::from(b8(data, at)?); // 세션 ID
    at += 2 + usize::from(be16(data, at)?); // 암호 목록
    at += 1 + usize::from(b8(data, at)?); // 압축 방식
    let end = at + 2 + usize::from(be16(data, at)?);
    at += 2;
    for _ in 0..64 {
        if at + 4 > end {
            break;
        }
        let (kind, len) = (be16(data, at)?, usize::from(be16(data, at + 2)?));
        if kind == 0 {
            let name_len = usize::from(be16(data, at + 7)?);
            let name = data.get(at + 9..at + 9 + name_len)?;
            return Some(name.iter().take(253).map(|&c| if c.is_ascii_alphanumeric() || c == b'.' || c == b'-' || c == b'_' { c as char } else { '?' }).collect());
        }
        at += 4 + len;
    }
    None
}

fn dhcp_kind(data: &[u8]) -> Option<&'static str> {
    if data.get(236..240)? != [0x63, 0x82, 0x53, 0x63] {
        return None;
    }
    let mut at = 240;
    for _ in 0..64 {
        let code = b8(data, at)?;
        if code == 0xff {
            break;
        }
        if code == 0 {
            at += 1;
            continue;
        }
        let len = usize::from(b8(data, at + 1)?);
        if code == 53 {
            return Some(match b8(data, at + 2)? {
                1 => "Discover (주소 요청 시작)",
                2 => "Offer (주소 제안)",
                3 => "Request (주소 요청)",
                4 => "Decline (거절)",
                5 => "ACK (주소 확정)",
                6 => "NAK (주소 거부)",
                7 => "Release (주소 반납)",
                8 => "Inform",
                _ => "기타",
            });
        }
        at += 2 + len;
    }
    None
}

fn dns_type(t: u16) -> String {
    match t {
        1 => "A".into(),
        2 => "NS".into(),
        5 => "CNAME".into(),
        6 => "SOA".into(),
        12 => "PTR".into(),
        15 => "MX".into(),
        16 => "TXT".into(),
        28 => "AAAA".into(),
        33 => "SRV".into(),
        65 => "HTTPS".into(),
        255 => "ANY".into(),
        n => format!("종류 {n}"),
    }
}

fn dns_rcode(r: u8) -> String {
    match r {
        0 => "정상".into(),
        1 => "형식 오류(FORMERR)".into(),
        2 => "서버 실패(SERVFAIL)".into(),
        3 => "그런 이름 없음(NXDOMAIN)".into(),
        5 => "거부(REFUSED)".into(),
        n => format!("오류 코드 {n}"),
    }
}

/// DNS 이름 읽기. 압축 포인터는 몇 번까지만 따라간다.
fn dns_name(msg: &[u8], mut at: usize) -> Option<(String, usize)> {
    let mut name = String::new();
    let mut after = None;
    let mut jumps = 0;
    loop {
        let len = usize::from(b8(msg, at)?);
        if len == 0 {
            at += 1;
            break;
        }
        if len & 0xc0 == 0xc0 {
            after.get_or_insert(at + 2);
            jumps += 1;
            if jumps > 8 {
                return None;
            }
            at = usize::from(be16(msg, at)? & 0x3fff);
            continue;
        }
        if len > 63 || name.len() > 255 {
            return None;
        }
        let label = msg.get(at + 1..at + 1 + len)?;
        if !name.is_empty() {
            name.push('.');
        }
        name.extend(label.iter().map(|&c| if c.is_ascii_alphanumeric() || c == b'-' || c == b'_' { c as char } else { '?' }));
        at += 1 + len;
    }
    Some((if name.is_empty() { ".".into() } else { name }, after.unwrap_or(at)))
}

/// (요약, 질의 종류, 답 개수)
fn dns(msg: &[u8], out: &mut Out) -> Option<(Dns, String, u16)> {
    let (id, flags, questions, answers) = (be16(msg, 0)?, be16(msg, 2)?, be16(msg, 4)?, be16(msg, 6)?);
    let response = flags & 0x8000 != 0;
    let rcode = (flags & 0x000f) as u8;
    let (name, kind) = if questions > 0 {
        let (name, next) = dns_name(msg, 12)?;
        (name, dns_type(be16(msg, next)?))
    } else {
        (String::new(), String::new())
    };
    out.layer("DNS");
    out.put("번호", || id.to_string());
    out.put("방향", || if response { "응답".into() } else { "질의".into() });
    out.put("이름", || name.clone());
    out.put("종류", || kind.clone());
    if response {
        out.put("결과", || dns_rcode(rcode));
        out.put("답 개수", || answers.to_string());
    }
    Some((Dns { id, response, rcode, name }, kind, answers))
}

fn dns_info(d: &Dns, kind: &str, answers: u16) -> String {
    match (d.response, d.rcode) {
        (false, _) => format!("질의 {kind} {}", d.name),
        (true, 0) => format!("응답 {kind} {} (답 {answers}개)", d.name),
        (true, r) => format!("응답 {kind} {} → {}", d.name, dns_rcode(r)),
    }
}

// ------------------------------------------------------------------ 파일 전체 분석

struct Rec {
    off: usize,
    cap: u32,
    wire: u32,
    time: f64,
    src: String,
    dst: String,
    sport: u16,
    dport: u16,
    proto: String,
    info: String,
    mark: &'static str,
}

struct Loaded {
    name: String,
    stamp: (u64, Option<SystemTime>),
    data: Vec<u8>,
    link: u32,
    recs: Vec<Rec>,
    analysis: Analysis,
}

fn link_name(link: u32) -> String {
    match link {
        1 => "이더넷".into(),
        113 => "리눅스 any (SLL)".into(),
        276 => "리눅스 any (SLL2)".into(),
        12 | 101 | 228 | 229 => "IP".into(),
        n => format!("모르는 링크 형식 {n}"),
    }
}

/// 여러 이름을 몇 개만 골라 한 줄로
fn some_of(items: &[String], max: usize) -> String {
    let mut seen = HashSet::new();
    let unique: Vec<&String> = items.iter().filter(|i| seen.insert(i.as_str())).collect();
    let mut text = unique.iter().take(max).map(|s| s.as_str()).collect::<Vec<_>>().join(", ");
    if unique.len() > max {
        text.push_str(&format!(" 외 {}곳", unique.len() - max));
    }
    text
}

fn analyze(name: &str, data: &[u8]) -> Result<(u32, f64, Vec<Rec>, Analysis), String> {
    let (little, nanos) = match data.get(..4) {
        Some([0xd4, 0xc3, 0xb2, 0xa1]) => (true, false),
        Some([0xa1, 0xb2, 0xc3, 0xd4]) => (false, false),
        Some([0x4d, 0x3c, 0xb2, 0xa1]) => (true, true),
        Some([0xa1, 0xb2, 0x3c, 0x4d]) => (false, true),
        Some([0x0a, 0x0d, 0x0d, 0x0a]) => return Err("pcapng 형식은 아직 읽지 못합니다. NetX가 저장한 캡처(pcap)만 열 수 있습니다.".into()),
        _ => return Err("캡처 파일(pcap)이 아닙니다.".into()),
    };
    let u32_at = |at: usize| data.get(at..at + 4).and_then(|b| <[u8; 4]>::try_from(b).ok()).map(|b| if little { u32::from_le_bytes(b) } else { u32::from_be_bytes(b) });
    let link = u32_at(20).ok_or("캡처 파일의 머리 부분이 잘려 있습니다.")? & 0x0fff_ffff;

    let mut recs: Vec<Rec> = Vec::new();
    let mut out = Out { detail: false, layers: Vec::new() };
    let (mut first, mut last, mut bytes) = (None, 0.0, 0u64);
    let (mut snapped, mut unknown) = (0u64, 0u64);
    let mut protocols: HashMap<String, (u64, u64)> = HashMap::new();
    let mut talks: HashMap<(String, String), (u64, u64)> = HashMap::new();

    // 문제 찾기에 쓰는 기억
    let mut segments: HashSet<u64> = HashSet::new();
    // 접속 시도: (보낸 쪽 → 받는 쪽) 해시 → (SYN 줄 번호들, 답이 왔는지, 받는 쪽 글자)
    let mut tries: HashMap<u64, (Vec<usize>, bool, String)> = HashMap::new();
    let mut asked: HashMap<(u16, String), usize> = HashMap::new();
    let (mut tcp_data, mut retransmits, mut resets, mut zero_windows, mut unreachables) = (0u64, 0u64, 0u64, 0u64, 0u64);
    let mut reset_from: Vec<String> = Vec::new();
    let mut dns_failed: Vec<String> = Vec::new();

    let mut at = 24;
    let mut cut = false;
    loop {
        let (Some(sec), Some(frac), Some(caplen), Some(wire)) = (u32_at(at), u32_at(at + 4), u32_at(at + 8), u32_at(at + 12)) else { break };
        let Some(frame) = data.get(at + 16..at + 16 + caplen as usize) else { break };
        if recs.len() >= MAX_PACKETS {
            cut = true;
            break;
        }
        let off = at + 16;
        at += 16 + caplen as usize;
        let time = f64::from(sec) + f64::from(frac) / if nanos { 1e9 } else { 1e6 };
        let start = *first.get_or_insert(time);
        last = time;
        bytes += u64::from(wire);
        if caplen < wire {
            snapped += 1;
        }

        let seen = decode(link, frame, &mut out);
        if seen.proto == "?" {
            unknown += 1;
        }
        let row = recs.len();
        let mut mark = "";
        if let Some(t) = seen.tcp {
            let (syn, ack, rst, fin) = (t.flags & 0x02 != 0, t.flags & 0x10 != 0, t.flags & 0x04 != 0, t.flags & 0x01 != 0);
            if t.len > 0 {
                tcp_data += 1;
            }
            // 같은 조각(또는 같은 접속 요청)이 다시 보이면 재전송이다.
            if (t.len > 0 || syn) && !segments.insert(hash_of(&(&seen.src, &seen.dst, seen.sport, seen.dport, t.seq, syn))) {
                mark = "retransmit";
                retransmits += 1;
            }
            if t.window == 0 && !syn && !rst && !fin {
                mark = "zero-window";
                zero_windows += 1;
            }
            if rst {
                mark = "reset";
                resets += 1;
                reset_from.push(format!("{}:{}", seen.src, seen.sport));
            }
            // 반대 방향 패킷이 하나라도 오면 그 접속 시도는 답을 받은 것이다.
            if let Some(t) = tries.get_mut(&hash_of(&(&seen.dst, &seen.src, seen.dport, seen.sport))) {
                t.1 = true;
            }
            if syn && !ack {
                tries.entry(hash_of(&(&seen.src, &seen.dst, seen.sport, seen.dport))).or_insert_with(|| (Vec::new(), false, format!("{}:{}", seen.dst, seen.dport))).0.push(row);
            }
        }
        if let Some(d) = &seen.dns {
            if d.response {
                asked.remove(&(d.id, d.name.clone()));
                if d.rcode != 0 {
                    mark = "dns-error";
                    dns_failed.push(format!("{} ({})", d.name, dns_rcode(d.rcode)));
                }
            } else {
                asked.insert((d.id, d.name.clone()), row);
            }
        }
        if seen.unreachable {
            mark = "unreachable";
            unreachables += 1;
        }

        let p = protocols.entry(seen.proto.clone()).or_default();
        p.0 += 1;
        p.1 += u64::from(wire);
        if seen.ip {
            let pair = if seen.src <= seen.dst { (seen.src.clone(), seen.dst.clone()) } else { (seen.dst.clone(), seen.src.clone()) };
            let t = talks.entry(pair).or_default();
            t.0 += 1;
            t.1 += u64::from(wire);
        }
        recs.push(Rec { off, cap: caplen, wire, time: time - start, src: seen.src, dst: seen.dst, sport: seen.sport, dport: seen.dport, proto: seen.proto, info: seen.info, mark });
    }
    if recs.is_empty() {
        return Err("캡처 파일에 패킷이 없습니다.".into());
    }
    let first = first.unwrap_or(0.0);
    let seconds = (last - first).max(0.0);

    // 답 없는 접속 시도, 답 없는 DNS 질의 (캡처가 끝나기 직전 2초의 것은 답을 기다리는 중일 수 있어 뺀다)
    let fresh = |row: usize| recs.get(row).map(|r| r.time > seconds - 2.0).unwrap_or(true);
    let mut silent: Vec<String> = Vec::new();
    let mut unanswered_rows: Vec<usize> = Vec::new();
    let mut silent_tries: Vec<&(Vec<usize>, bool, String)> = tries.values().filter(|t| !t.1 && !t.0.first().map(|r| fresh(*r)).unwrap_or(true)).collect();
    silent_tries.sort_by_key(|t| t.0.first().copied());
    for t in silent_tries {
        silent.push(t.2.clone());
        unanswered_rows.extend(&t.0);
    }
    let mut lost: Vec<(usize, String)> = asked.into_iter().filter(|(_, row)| !fresh(*row)).map(|((_, name), row)| (row, name)).collect();
    lost.sort();
    for row in unanswered_rows {
        if let Some(r) = recs.get_mut(row) {
            r.mark = "unanswered";
        }
    }
    for (row, _) in &lost {
        if let Some(r) = recs.get_mut(*row) {
            r.mark = "dns-unanswered";
        }
    }

    let mut findings = Vec::new();
    if !silent.is_empty() {
        let n = { let mut s = HashSet::new(); silent.iter().filter(|x| s.insert(x.as_str())).count() } as u64;
        findings.push(Finding {
            light: "red",
            title: format!("답 없는 접속 시도 {n}곳"),
            text: format!("접속을 요청(SYN)했는데 아무 답이 없습니다: {}. 방화벽이 조용히 버리고 있거나 상대 장비가 꺼져 있습니다.", some_of(&silent, 4)),
            count: n,
            mark: "unanswered",
        });
    }
    if !dns_failed.is_empty() {
        findings.push(Finding {
            light: "red",
            title: format!("DNS 실패 {}건", dns_failed.len()),
            text: format!("DNS 서버가 오류로 답했습니다: {}. 이름 철자나 DNS 서버 설정을 확인하세요.", some_of(&dns_failed, 4)),
            count: dns_failed.len() as u64,
            mark: "dns-error",
        });
    }
    if !lost.is_empty() {
        let names: Vec<String> = lost.iter().map(|(_, n)| n.clone()).collect();
        findings.push(Finding {
            light: "yellow",
            title: format!("답 없는 DNS 질의 {}건", lost.len()),
            text: format!("질의를 보냈는데 응답이 캡처에 없습니다: {}. DNS 서버가 느리거나 막혀 있을 수 있습니다.", some_of(&names, 4)),
            count: lost.len() as u64,
            mark: "dns-unanswered",
        });
    }
    if retransmits > 0 {
        let heavy = tcp_data > 0 && retransmits * 100 >= tcp_data * 2;
        findings.push(Finding {
            light: if heavy { "red" } else { "yellow" },
            title: format!("재전송 {retransmits}건"),
            text: format!(
                "같은 데이터를 다시 보낸 패킷입니다(데이터 패킷 {tcp_data}개 중 {retransmits}개). {}",
                if heavy { "비율이 높습니다. 구간에서 패킷이 사라지고 있습니다(회선 품질, 혼잡, 무선 신호)." } else { "조금은 정상입니다. 계속 늘면 회선 품질을 확인하세요." }
            ),
            count: retransmits,
            mark: "retransmit",
        });
    }
    if resets > 0 {
        findings.push(Finding {
            light: "yellow",
            title: format!("연결 끊음(RST) {resets}건"),
            text: format!("연결을 거부하거나 강제로 끊은 패킷입니다. 보낸 곳: {}. 닫힌 포트로 접속했거나 방화벽·서버가 끊은 것입니다.", some_of(&reset_from, 4)),
            count: resets,
            mark: "reset",
        });
    }
    if unreachables > 0 {
        findings.push(Finding {
            light: "yellow",
            title: format!("ICMP 오류 알림 {unreachables}건"),
            text: "중간 장비나 상대가 \"갈 수 없음\" 또는 \"시간 초과\"를 알려 왔습니다. 목록에서 어느 주소·포트인지 확인하세요.".into(),
            count: unreachables,
            mark: "unreachable",
        });
    }
    if zero_windows > 0 {
        findings.push(Finding {
            light: "yellow",
            title: format!("수신 버퍼 가득 참 {zero_windows}건"),
            text: "받는 쪽이 \"더 못 받는다\"(창 크기 0)고 알렸습니다. 받는 프로그램이 느리거나 멈춰 있습니다.".into(),
            count: zero_windows,
            mark: "zero-window",
        });
    }

    let mut notes = Vec::new();
    if snapped > 0 {
        notes.push(format!("패킷 {snapped}개는 앞부분만 저장되어 있습니다(캡처할 때 크기를 줄였습니다). 내용 일부는 보이지 않습니다."));
    }
    if cut {
        notes.push(format!("패킷이 많아 앞의 {MAX_PACKETS}개만 읽었습니다."));
    }
    if unknown > 0 {
        notes.push(format!("해석하지 못한 패킷이 {unknown}개 있습니다."));
    }

    let mut protocols: Vec<Share> = protocols.into_iter().map(|(label, (packets, bytes))| Share { label, packets, bytes }).collect();
    protocols.sort_by(|a, b| b.packets.cmp(&a.packets).then_with(|| a.label.cmp(&b.label)));
    protocols.truncate(12);
    let mut talks: Vec<Talk> = talks.into_iter().map(|((a, b), (packets, bytes))| Talk { a, b, packets, bytes }).collect();
    talks.sort_by(|x, y| y.bytes.cmp(&x.bytes).then_with(|| x.a.cmp(&y.a)).then_with(|| x.b.cmp(&y.b)));
    talks.truncate(10);

    let analysis =
        Analysis { name: name.to_string(), size: data.len() as u64, started: first, seconds, packets: recs.len() as u64, bytes, link: link_name(link), protocols, talks, findings, notes };
    Ok((link, first, recs, analysis))
}

// ------------------------------------------------------------------ 파일 열기와 조회

/// 작업 폴더 안의 캡처 파일 이름인지. 경로가 아니라 이름만 받는다.
pub fn valid_name(name: &str) -> bool {
    name.len() <= 100 && name.ends_with(".pcap") && !name.starts_with('.') && name.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.'))
}

pub fn list(work: &Path) -> Vec<CaptureFile> {
    let Ok(dir) = std::fs::read_dir(work) else { return Vec::new() };
    let mut files: Vec<CaptureFile> = dir
        .flatten()
        .filter_map(|e| {
            let name = e.file_name().to_str()?.to_string();
            let meta = e.metadata().ok()?;
            (valid_name(&name) && meta.is_file()).then(|| CaptureFile {
                name,
                size: meta.len(),
                modified: meta.modified().ok().and_then(|t| t.duration_since(SystemTime::UNIX_EPOCH).ok()).map(|d| d.as_secs()).unwrap_or(0),
            })
        })
        .collect();
    files.sort_by(|a, b| b.modified.cmp(&a.modified).then_with(|| b.name.cmp(&a.name)));
    files.truncate(200);
    files
}

static LOADED: Mutex<Option<Loaded>> = Mutex::new(None);

/// 파일을 읽어 분석해 두고(같은 파일이면 다시 읽지 않는다) 그 결과로 `f`를 부른다.
fn with<T>(work: &Path, name: &str, f: impl FnOnce(&Loaded) -> Result<T, String>) -> Result<T, String> {
    if !valid_name(name) {
        return Err("캡처 파일 이름이 올바르지 않습니다.".into());
    }
    let path = work.join(name);
    let meta = std::fs::metadata(&path).map_err(|_| "캡처 파일을 찾지 못했습니다.".to_string())?;
    if !meta.is_file() {
        return Err("캡처 파일을 찾지 못했습니다.".into());
    }
    if meta.len() > MAX_FILE {
        return Err(format!("파일이 너무 큽니다({}MB). 64MB까지 열 수 있습니다. [폴더 열기]로 Wireshark에서 여세요.", meta.len() / 1024 / 1024));
    }
    let stamp = (meta.len(), meta.modified().ok());
    let mut guard = LOADED.lock().unwrap_or_else(|e| e.into_inner());
    if !guard.as_ref().is_some_and(|l| l.name == name && l.stamp == stamp) {
        *guard = None;
        let data = std::fs::read(&path).map_err(|_| "캡처 파일을 읽지 못했습니다.".to_string())?;
        let (link, _, recs, analysis) = analyze(name, &data)?;
        *guard = Some(Loaded { name: name.to_string(), stamp, data, link, recs, analysis });
    }
    f(guard.as_ref().ok_or("캡처 파일을 열지 못했습니다.")?)
}

pub fn open(work: &Path, name: &str) -> Result<Analysis, String> {
    with(work, name, |l| Ok(l.analysis.clone()))
}

/// 거르는 글: 띄어 쓴 낱말이 모두 맞아야 한다. 낱말은 주소(앞부분), 포트 번호, 프로토콜 이름, 설명 속 글자 중 하나와 맞으면 된다.
fn matches(r: &Rec, words: &[String]) -> bool {
    words.iter().all(|w| {
        if let Ok(port) = w.parse::<u16>() {
            if r.sport == port || r.dport == port {
                return true;
            }
        }
        r.proto.to_lowercase() == *w || r.src.starts_with(w.as_str()) || r.dst.starts_with(w.as_str()) || r.info.to_lowercase().contains(w.as_str())
    })
}

/// `mark`: 빈 글자는 전부, "any"는 문제 표시가 있는 것 전부, 그 밖은 그 표시만.
pub fn rows(work: &Path, name: &str, text: &str, mark: &str, offset: usize, limit: usize) -> Result<Page, String> {
    let words: Vec<String> = text.split_whitespace().take(8).map(|w| w.chars().take(64).collect::<String>().to_lowercase()).collect();
    with(work, name, |l| {
        let hits = l.recs.iter().enumerate().filter(|(_, r)| match mark {
            "" => true,
            "any" => !r.mark.is_empty(),
            m => r.mark == m,
        });
        let hits: Vec<(usize, &Rec)> = hits.filter(|(_, r)| matches(r, &words)).collect();
        let rows = hits
            .iter()
            .skip(offset)
            .take(limit.clamp(1, PAGE_MAX))
            .map(|(i, r)| Row { no: *i as u32 + 1, time: r.time, src: r.src.clone(), dst: r.dst.clone(), proto: r.proto.clone(), len: r.wire, info: r.info.clone(), mark: r.mark })
            .collect();
        Ok(Page { total: hits.len(), rows })
    })
}

fn hex_lines(frame: &[u8]) -> Vec<String> {
    frame
        .chunks(16)
        .enumerate()
        .map(|(i, chunk)| {
            let hex: Vec<String> = chunk.iter().map(|b| format!("{b:02x}")).collect();
            format!("{:04x}  {:<47}  {}", i * 16, hex.join(" "), clean(chunk, 16))
        })
        .collect()
}

pub fn packet(work: &Path, name: &str, no: u32) -> Result<Detail, String> {
    with(work, name, |l| {
        let r = (no as usize).checked_sub(1).and_then(|i| l.recs.get(i)).ok_or("그 번호의 패킷이 없습니다.")?;
        let frame = l.data.get(r.off..r.off + r.cap as usize).ok_or("패킷을 읽지 못했습니다.")?;
        let mut out = Out { detail: true, layers: Vec::new() };
        decode(l.link, frame, &mut out);
        let shown = frame.get(..frame.len().min(HEX_MAX)).unwrap_or(frame);
        Ok(Detail { no, time: r.time, len: r.wire, captured: r.cap, layers: out.layers, hex: hex_lines(shown), cut: frame.len() > HEX_MAX })
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const ME: [u8; 4] = [192, 168, 0, 10];
    const WEB: [u8; 4] = [93, 184, 216, 34];
    const DNS: [u8; 4] = [8, 8, 8, 8];

    fn ip(proto: u8, src: [u8; 4], dst: [u8; 4], l4: &[u8]) -> Vec<u8> {
        let total = (20 + l4.len()) as u16;
        let mut p = vec![0x45, 0, (total >> 8) as u8, total as u8, 0, 0, 0, 0, 64, proto, 0, 0];
        p.extend_from_slice(&src);
        p.extend_from_slice(&dst);
        p.extend_from_slice(l4);
        p
    }

    fn tcp(src: [u8; 4], dst: [u8; 4], sport: u16, dport: u16, seq: u32, flags: u8, window: u16, payload: &[u8]) -> Vec<u8> {
        let mut t = Vec::new();
        t.extend_from_slice(&sport.to_be_bytes());
        t.extend_from_slice(&dport.to_be_bytes());
        t.extend_from_slice(&seq.to_be_bytes());
        t.extend_from_slice(&[0, 0, 0, 0, 0x50, flags]);
        t.extend_from_slice(&window.to_be_bytes());
        t.extend_from_slice(&[0, 0, 0, 0]);
        t.extend_from_slice(payload);
        ip(6, src, dst, &t)
    }

    fn udp(src: [u8; 4], dst: [u8; 4], sport: u16, dport: u16, payload: &[u8]) -> Vec<u8> {
        let mut u = Vec::new();
        u.extend_from_slice(&sport.to_be_bytes());
        u.extend_from_slice(&dport.to_be_bytes());
        u.extend_from_slice(&((8 + payload.len()) as u16).to_be_bytes());
        u.extend_from_slice(&[0, 0]);
        u.extend_from_slice(payload);
        ip(17, src, dst, &u)
    }

    fn dns_msg(id: u16, response: bool, rcode: u8, name: &str) -> Vec<u8> {
        let mut m = id.to_be_bytes().to_vec();
        m.extend_from_slice(&[if response { 0x81 } else { 0x01 }, rcode, 0, 1, 0, u8::from(response && rcode == 0), 0, 0, 0, 0]);
        for label in name.split('.') {
            m.push(label.len() as u8);
            m.extend_from_slice(label.as_bytes());
        }
        m.extend_from_slice(&[0, 0, 1, 0, 1]);
        m
    }

    fn ethernet(ip: &[u8]) -> Vec<u8> {
        [&[2, 0, 0, 0, 0, 1][..], &[2, 0, 0, 0, 0, 2], &[0x08, 0x00], ip].concat()
    }

    fn pcap(link: u32, frames: &[(u32, Vec<u8>)]) -> Vec<u8> {
        let mut out = vec![0xd4, 0xc3, 0xb2, 0xa1, 2, 0, 4, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 4, 0];
        out.extend_from_slice(&link.to_le_bytes());
        for (sec, frame) in frames {
            out.extend_from_slice(&sec.to_le_bytes());
            out.extend_from_slice(&0u32.to_le_bytes());
            out.extend_from_slice(&(frame.len() as u32).to_le_bytes());
            out.extend_from_slice(&(frame.len() as u32).to_le_bytes());
            out.extend_from_slice(frame);
        }
        out
    }

    fn client_hello(name: &str) -> Vec<u8> {
        let mut ext = vec![0, 0];
        ext.extend_from_slice(&((name.len() + 5) as u16).to_be_bytes());
        ext.extend_from_slice(&((name.len() + 3) as u16).to_be_bytes());
        ext.push(0);
        ext.extend_from_slice(&(name.len() as u16).to_be_bytes());
        ext.extend_from_slice(name.as_bytes());
        let mut body = vec![3, 3];
        body.extend_from_slice(&[7; 32]);
        body.extend_from_slice(&[0, 0, 2, 0x13, 0x01, 1, 0]);
        body.extend_from_slice(&(ext.len() as u16).to_be_bytes());
        body.extend_from_slice(&ext);
        let mut hs = vec![1, 0];
        hs.extend_from_slice(&(body.len() as u16).to_be_bytes());
        hs.extend_from_slice(&body);
        let mut rec = vec![0x16, 3, 1];
        rec.extend_from_slice(&(hs.len() as u16).to_be_bytes());
        rec.extend_from_slice(&hs);
        rec
    }

    fn sample() -> Vec<u8> {
        let dead = [10, 0, 0, 99];
        let closed = [10, 0, 0, 5];
        pcap(
            1,
            &[
                (100, ethernet(&udp(ME, DNS, 50000, 53, &dns_msg(7, false, 0, "example.com")))),
                (100, ethernet(&udp(DNS, ME, 53, 50000, &dns_msg(7, true, 0, "example.com")))),
                (100, ethernet(&udp(ME, DNS, 50001, 53, &dns_msg(8, false, 0, "nope.invalid")))),
                (100, ethernet(&udp(DNS, ME, 53, 50001, &dns_msg(8, true, 3, "nope.invalid")))),
                (100, ethernet(&udp(ME, DNS, 50002, 53, &dns_msg(9, false, 0, "lost.example")))),
                // 정상 접속과 TLS, 재전송
                (101, ethernet(&tcp(ME, WEB, 40000, 443, 1, 0x02, 64240, &[]))),
                (101, ethernet(&tcp(WEB, ME, 443, 40000, 9, 0x12, 65535, &[]))),
                (101, ethernet(&tcp(ME, WEB, 40000, 443, 2, 0x18, 64240, &client_hello("example.com")))),
                (102, ethernet(&tcp(ME, WEB, 40000, 443, 2, 0x18, 64240, &client_hello("example.com")))),
                // 답 없는 접속 시도 (같은 SYN 두 번)
                (103, ethernet(&tcp(ME, dead, 40001, 8080, 5, 0x02, 64240, &[]))),
                (104, ethernet(&tcp(ME, dead, 40001, 8080, 5, 0x02, 64240, &[]))),
                // 닫힌 포트: RST
                (105, ethernet(&tcp(ME, closed, 40002, 23, 5, 0x02, 64240, &[]))),
                (105, ethernet(&tcp(closed, ME, 23, 40002, 0, 0x14, 0, &[]))),
                // HTTP, 창 크기 0, ICMP 도달 불가
                (106, ethernet(&tcp(ME, WEB, 40003, 80, 1, 0x18, 64240, b"GET /a\x1b[31m HTTP/1.1\r\nHost: x\r\n\r\n"))),
                (106, ethernet(&tcp(WEB, ME, 80, 40003, 1, 0x10, 0, &[]))),
                (107, ethernet(&ip(1, closed, ME, &[3, 3, 0, 0, 0, 0, 0, 0]))),
                (120, ethernet(&ip(1, ME, DNS, &[8, 0, 0, 0, 0, 1, 0, 1]))),
            ],
        )
    }

    #[test]
    fn finds_problems_and_describes_packets() {
        let (_, _, recs, a) = analyze("capture-1.pcap", &sample()).unwrap();
        assert_eq!((a.packets, a.seconds, a.link.as_str()), (17, 20.0, "이더넷"));
        let info = |i: usize| recs[i].info.as_str();
        assert_eq!(info(0), "질의 A example.com");
        assert_eq!(info(1), "응답 A example.com (답 1개)");
        assert!(info(3).contains("NXDOMAIN"));
        assert_eq!(info(5), "40000 → 443 [SYN] Seq=1 Ack=0 Win=64240 Len=0");
        assert_eq!((recs[7].proto.as_str(), info(7)), ("TLS", "40000 → 443 TLS 접속 시작(ClientHello) 이름=example.com"));
        assert_eq!((recs[13].proto.as_str(), info(13)), ("HTTP", "40003 → 80 GET /a.[31m HTTP/1.1"), "제어 문자는 점으로");
        assert_eq!(info(15), "도달 불가: 포트가 닫혀 있음");
        assert_eq!(info(16), "핑 요청 id=1 seq=1");

        let marks: Vec<&str> = recs.iter().map(|r| r.mark).collect();
        assert_eq!(marks[3], "dns-error");
        assert_eq!(marks[4], "dns-unanswered");
        assert_eq!(marks[8], "retransmit");
        assert_eq!((marks[9], marks[10]), ("unanswered", "unanswered"));
        assert_eq!((marks[11], marks[12]), ("", "reset"));
        assert_eq!(marks[14], "zero-window");
        assert_eq!(marks[15], "unreachable");
        assert_eq!(marks[5], "", "답을 받은 접속은 문제가 아니다");

        let titles: Vec<&str> = a.findings.iter().map(|f| f.title.as_str()).collect();
        assert_eq!(titles, ["답 없는 접속 시도 1곳", "DNS 실패 1건", "답 없는 DNS 질의 1건", "재전송 2건", "연결 끊음(RST) 1건", "ICMP 오류 알림 1건", "수신 버퍼 가득 참 1건"]);
        assert!(a.findings[0].text.contains("10.0.0.99:8080"));
        assert_eq!(a.protocols[0].label, "TCP");
        assert!(a.protocols.iter().any(|p| p.label == "DNS" && p.packets == 5));
        assert_eq!(a.talks[0].packets + a.talks[1].packets + a.talks[2].packets + a.talks[3].packets, 17);
    }

    #[test]
    fn lists_filters_and_shows_detail() {
        let dir = std::env::temp_dir().join(format!("netx-capview-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("capture-9.pcap"), sample()).unwrap();
        std::fs::write(dir.join("notes.txt"), "x").unwrap();
        assert_eq!(list(&dir).iter().map(|f| f.name.as_str()).collect::<Vec<_>>(), ["capture-9.pcap"]);

        let name = "capture-9.pcap";
        assert_eq!(open(&dir, name).unwrap().packets, 17);
        let nos = |text: &str, mark: &str| rows(&dir, name, text, mark, 0, 100).unwrap().rows.iter().map(|r| r.no).collect::<Vec<_>>();
        assert_eq!(nos("", "").len(), 17);
        assert_eq!(nos("dns", ""), [1, 2, 3, 4, 5]);
        assert_eq!(nos("443", ""), [6, 7, 8, 9]);
        assert_eq!(nos("10.0.0.99", ""), [10, 11]);
        assert_eq!(nos("tcp 10.0.0.5", ""), [12, 13]);
        assert_eq!(nos("NXDOMAIN", ""), [4]);
        assert_eq!(nos("", "any"), [4, 5, 9, 10, 11, 13, 15, 16]);
        assert_eq!(nos("", "reset"), [13]);
        let page = rows(&dir, name, "", "", 15, 1).unwrap();
        assert_eq!((page.total, page.rows.len(), page.rows[0].no), (17, 1, 16));

        let d = packet(&dir, name, 8).unwrap();
        assert_eq!(d.layers.iter().map(|l| l.name.as_str()).collect::<Vec<_>>(), ["이더넷", "IPv4", "TCP", "TLS"]);
        assert!(d.layers[3].fields.iter().any(|f| f.name == "접속 이름(SNI)" && f.value == "example.com"));
        assert!(d.hex[0].starts_with("0000  02 00 00 00 00 01 02 00 00 00 00 02 08 00 45 00"));
        assert!(packet(&dir, name, 0).is_err() && packet(&dir, name, 18).is_err());

        for bad in ["../capture-9.pcap", "capture 9.pcap", ".pcap", "notes.txt", "a/b.pcap", "c:\\x.pcap"] {
            assert!(open(&dir, bad).is_err(), "{bad}");
        }
        assert!(open(&dir, "capture-none.pcap").is_err());
        std::fs::write(dir.join("ng.pcap"), [0x0a, 0x0d, 0x0d, 0x0a, 0, 0, 0, 0]).unwrap();
        assert!(open(&dir, "ng.pcap").unwrap_err().contains("pcapng"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn other_links_and_ipv6() {
        // SLL2 + IPv6 + UDP(mDNS)
        let mut v6 = vec![0x60, 0, 0, 0, 0, 8, 17, 255];
        v6.extend_from_slice(&[0xfe, 0x80, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 1]);
        v6.extend_from_slice(&[0xff, 0x02, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0xfb]);
        v6.extend_from_slice(&[0x14, 0xe9, 0x14, 0xe9, 0, 8, 0, 0]);
        let mut sll2 = vec![0x86, 0xdd];
        sll2.extend_from_slice(&[0; 18]);
        sll2.extend_from_slice(&v6);
        let arp = [&[0xff; 6][..], &[2, 0, 0, 0, 0, 2], &[0x08, 0x06], &[0, 1, 8, 0, 6, 4, 0, 1], &[2, 0, 0, 0, 0, 2], &[192, 168, 0, 10], &[0; 6], &[192, 168, 0, 1]].concat();

        let (_, _, recs, _) = analyze("a.pcap", &pcap(276, &[(1, sll2)])).unwrap();
        assert_eq!((recs[0].src.as_str(), recs[0].dst.as_str(), recs[0].proto.as_str()), ("fe80::1", "ff02::fb", "mDNS"));
        let (_, _, recs, a) = analyze("a.pcap", &pcap(1, &[(1, arp)])).unwrap();
        assert_eq!((recs[0].proto.as_str(), recs[0].info.as_str()), ("ARP", "192.168.0.1은(는) 누구입니까? 192.168.0.10에게 알려 주세요"));
        assert!(a.talks.is_empty());
        // 모르는 링크 형식도 목록에는 나온다
        let (_, _, recs, a) = analyze("a.pcap", &pcap(147, &[(1, vec![1, 2, 3])])).unwrap();
        assert_eq!(recs[0].proto, "?");
        assert_eq!(a.notes.len(), 1);
    }

    /// 깨진 입력과 무작위 입력에서 멈추거나 죽지 않아야 한다.
    #[test]
    fn broken_and_random_input_never_panics() {
        let good = sample();
        for cut in 0..good.len() {
            let _ = analyze("a.pcap", &good[..cut]);
        }
        let mut seed = 0x2545_f491_4f6c_dd1du64;
        let mut next = || {
            seed ^= seed << 13;
            seed ^= seed >> 7;
            seed ^= seed << 17;
            seed
        };
        for round in 0..3000 {
            let mut data = good.clone();
            for _ in 0..1 + round % 6 {
                let at = (next() as usize) % data.len();
                data[at] = next() as u8;
            }
            if let Ok((link, _, recs, _)) = analyze("a.pcap", &data) {
                for r in recs.iter().take(40) {
                    if let Some(frame) = data.get(r.off..r.off + r.cap as usize) {
                        let mut out = Out { detail: true, layers: Vec::new() };
                        decode(link, frame, &mut out);
                        hex_lines(frame.get(..frame.len().min(HEX_MAX)).unwrap_or(frame));
                    }
                }
            }
        }
        for link in [1u32, 113, 276, 101, 0, 999] {
            for _ in 0..400 {
                let frame: Vec<u8> = (0..(next() % 120) as usize).map(|_| next() as u8).collect();
                decode(link, &frame, &mut Out { detail: true, layers: Vec::new() });
            }
        }
    }
}
