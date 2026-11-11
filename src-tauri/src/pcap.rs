//! 캡처 파일(pcap)을 읽어 "패킷 하이웨이" 그림의 자료를 만든다.
//!
//! tcpdump가 쓴 옛 형식 pcap만 읽는다(pcapng는 읽지 않는다). 헤더만 본다: 누가 누구와, 어느 포트로, 몇 개·몇 바이트.
//! 패킷 내용(payload)은 보지 않는다. 깨진 파일은 읽은 데까지만 쓴다.

use std::collections::{HashMap, HashSet};
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};
use std::path::Path;

use serde::Serialize;

/// 읽을 파일 크기와 패킷 수의 상한
const MAX_FILE: u64 = 64 * 1024 * 1024;
const MAX_PACKETS: usize = 200_000;
/// 그림에 보일 상대 수
const MAX_FLOWS: usize = 12;

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct Flow {
    pub peer: String,
    pub port: u16,
    /// tcp, udp, icmp, other
    pub proto: &'static str,
    /// secure(암호화 통신), dns, plain(그 밖), trouble(재전송·연결 거부가 있음)
    pub class: &'static str,
    /// 내가 보낸 패킷 수, 받은 패킷 수
    pub sent: u64,
    pub received: u64,
    pub bytes: u64,
    /// 재전송과 RST의 수
    pub trouble: u64,
    pub kbps: f64,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct Highway {
    pub me: String,
    pub packets: u64,
    pub seconds: f64,
    pub flows: Vec<Flow>,
    /// 그림에 다 넣지 못한 상대의 수
    pub more: usize,
}

struct Packet {
    src: IpAddr,
    dst: IpAddr,
    proto: &'static str,
    sport: u16,
    dport: u16,
    /// (seq, 데이터 길이, RST 여부) — TCP만
    tcp: Option<(u32, usize, bool)>,
}

fn be16(b: &[u8], at: usize) -> Option<u16> {
    Some(u16::from_be_bytes([*b.get(at)?, *b.get(at + 1)?]))
}

/// 링크 계층 헤더를 벗겨 IP 패킷을 돌려준다.
fn ip_payload(link: u32, frame: &[u8]) -> Option<&[u8]> {
    match link {
        // 이더넷 (VLAN 꼬리표가 있으면 한 겹 더 벗긴다)
        1 => match be16(frame, 12)? {
            0x8100 => matches!(be16(frame, 16)?, 0x0800 | 0x86dd).then(|| frame.get(18..)).flatten(),
            0x0800 | 0x86dd => frame.get(14..),
            _ => None,
        },
        // 리눅스 "any" 인터페이스 (SLL, SLL2)
        113 => matches!(be16(frame, 14)?, 0x0800 | 0x86dd).then(|| frame.get(16..)).flatten(),
        276 => matches!(be16(frame, 0)?, 0x0800 | 0x86dd).then(|| frame.get(20..)).flatten(),
        // IP만
        12 | 101 | 228 | 229 => Some(frame),
        _ => None,
    }
}

fn parse_ip(ip: &[u8]) -> Option<Packet> {
    let (src, dst, next, rest): (IpAddr, IpAddr, u8, &[u8]) = match ip.first()? >> 4 {
        4 => {
            let header = usize::from(ip[0] & 0x0f) * 4;
            let a = ip.get(12..20)?;
            (Ipv4Addr::new(a[0], a[1], a[2], a[3]).into(), Ipv4Addr::new(a[4], a[5], a[6], a[7]).into(), *ip.get(9)?, ip.get(header.max(20)..)?)
        }
        6 => {
            let a = ip.get(8..40)?;
            (Ipv6Addr::from(<[u8; 16]>::try_from(&a[..16]).ok()?).into(), Ipv6Addr::from(<[u8; 16]>::try_from(&a[16..]).ok()?).into(), *ip.get(6)?, ip.get(40..)?)
        }
        _ => return None,
    };
    Some(match next {
        6 => {
            let header = usize::from(*rest.get(12)? >> 4) * 4;
            let seq = u32::from_be_bytes(rest.get(4..8)?.try_into().ok()?);
            let rst = rest.get(13)? & 0x04 != 0;
            Packet { src, dst, proto: "tcp", sport: be16(rest, 0)?, dport: be16(rest, 2)?, tcp: Some((seq, rest.len().saturating_sub(header), rst)) }
        }
        17 => Packet { src, dst, proto: "udp", sport: be16(rest, 0)?, dport: be16(rest, 2)?, tcp: None },
        1 | 58 => Packet { src, dst, proto: "icmp", sport: 0, dport: 0, tcp: None },
        _ => Packet { src, dst, proto: "other", sport: 0, dport: 0, tcp: None },
    })
}

fn class_of(proto: &str, port: u16) -> &'static str {
    match (proto, port) {
        (_, 53 | 5353 | 5355) => "dns",
        ("tcp", 443 | 22 | 853 | 993 | 995 | 465 | 636 | 8443 | 3389) | ("udp", 443 | 500 | 4500 | 51820) => "secure",
        _ => "plain",
    }
}

/// pcap 바이트에서 하이웨이 자료를 만든다. pcap이 아니거나 IP 패킷이 없으면 None.
pub fn highway(data: &[u8]) -> Option<Highway> {
    let magic = data.get(..4)?;
    // 바이트 순서와 시각 단위(마이크로초, 나노초)
    let (little, nanos) = match magic {
        [0xd4, 0xc3, 0xb2, 0xa1] => (true, false),
        [0xa1, 0xb2, 0xc3, 0xd4] => (false, false),
        [0x4d, 0x3c, 0xb2, 0xa1] => (true, true),
        [0xa1, 0xb2, 0x3c, 0x4d] => (false, true),
        _ => return None,
    };
    let u32_at = |at: usize| data.get(at..at + 4).map(|b| if little { u32::from_le_bytes([b[0], b[1], b[2], b[3]]) } else { u32::from_be_bytes([b[0], b[1], b[2], b[3]]) });
    let link = u32_at(20)? & 0x0fff_ffff;

    let mut packets = Vec::new();
    let (mut first, mut last) = (None, 0.0);
    let mut at = 24;
    while packets.len() < MAX_PACKETS {
        let (Some(sec), Some(frac), Some(caplen), Some(wire)) = (u32_at(at), u32_at(at + 4), u32_at(at + 8), u32_at(at + 12)) else { break };
        let Some(frame) = data.get(at + 16..at + 16 + caplen as usize) else { break };
        at += 16 + caplen as usize;
        let time = f64::from(sec) + f64::from(frac) / if nanos { 1e9 } else { 1e6 };
        first.get_or_insert(time);
        last = time;
        if let Some(p) = ip_payload(link, frame).and_then(parse_ip) {
            packets.push((p, u64::from(wire)));
        }
    }
    if packets.is_empty() {
        return None;
    }

    // 내 PC: 가장 많은 패킷에 나오는 주소
    let mut seen: HashMap<IpAddr, u32> = HashMap::new();
    for (p, _) in &packets {
        *seen.entry(p.src).or_default() += 1;
        *seen.entry(p.dst).or_default() += 1;
    }
    let me = *seen.iter().max_by_key(|(ip, n)| (**n, std::cmp::Reverse(**ip)))?.0;

    let seconds = (last - first.unwrap_or(last)).max(0.0);
    let mut flows: HashMap<(IpAddr, u16, &'static str), Flow> = HashMap::new();
    let mut segments: HashSet<(IpAddr, IpAddr, u16, u16, u32)> = HashSet::new();
    for (p, wire) in &packets {
        let (peer, outgoing) = if p.src == me { (p.dst, true) } else if p.dst == me { (p.src, false) } else { continue };
        // 서비스 포트: 상대 쪽 포트. 내가 서버면(상대 포트가 높고 내 포트가 낮으면) 내 포트.
        let (mine, theirs) = if outgoing { (p.sport, p.dport) } else { (p.dport, p.sport) };
        let port = if theirs >= 1024 && mine != 0 && mine < 1024 { mine } else { theirs };
        let flow = flows.entry((peer, port, p.proto)).or_insert_with(|| Flow { peer: peer.to_string(), port, proto: p.proto, class: class_of(p.proto, port), sent: 0, received: 0, bytes: 0, trouble: 0, kbps: 0.0 });
        if outgoing {
            flow.sent += 1;
        } else {
            flow.received += 1;
        }
        flow.bytes += wire;
        if let Some((seq, len, rst)) = p.tcp {
            // 같은 데이터 조각이 다시 보이면 재전송이다.
            if rst || (len > 0 && !segments.insert((p.src, p.dst, p.sport, p.dport, seq))) {
                flow.trouble += 1;
            }
        }
    }
    let mut flows: Vec<Flow> = flows.into_values().collect();
    for f in &mut flows {
        if f.trouble > 0 {
            f.class = "trouble";
        }
        f.kbps = if seconds > 0.0 { f.bytes as f64 * 8.0 / 1000.0 / seconds } else { 0.0 };
    }
    flows.sort_by(|a, b| (b.sent + b.received).cmp(&(a.sent + a.received)).then_with(|| a.peer.cmp(&b.peer)).then(a.port.cmp(&b.port)));
    let more = flows.len().saturating_sub(MAX_FLOWS);
    flows.truncate(MAX_FLOWS);
    Some(Highway { me: me.to_string(), packets: packets.len() as u64, seconds, flows, more })
}

/// 캡처 파일을 읽는다. 너무 크거나 읽을 수 없으면 None.
pub fn read(path: &Path) -> Option<Highway> {
    if std::fs::metadata(path).ok()?.len() > MAX_FILE {
        return None;
    }
    highway(&std::fs::read(path).ok()?)
}

/// tcpdump 인자에서 캡처 파일 이름(`-w` 값)을 찾는다. 경로가 아니라 이름만 받는다.
pub fn capture_name(argv: &[String]) -> Option<&str> {
    let name = argv.iter().position(|a| a == "-w").and_then(|i| argv.get(i + 1))?;
    (name.ends_with(".pcap") && !name.contains(['/', '\\']) && !name.starts_with('.')).then_some(name.as_str())
}

#[cfg(test)]
mod tests {
    use super::*;

    const ME: [u8; 4] = [192, 168, 0, 10];

    fn tcp(src: [u8; 4], dst: [u8; 4], sport: u16, dport: u16, seq: u32, flags: u8, payload: usize) -> Vec<u8> {
        let mut p = vec![0x45, 0, 0, 0, 0, 0, 0, 0, 64, 6, 0, 0];
        p.extend_from_slice(&src);
        p.extend_from_slice(&dst);
        p.extend_from_slice(&sport.to_be_bytes());
        p.extend_from_slice(&dport.to_be_bytes());
        p.extend_from_slice(&seq.to_be_bytes());
        p.extend_from_slice(&[0, 0, 0, 0, 0x50, flags, 0, 0, 0, 0, 0, 0]);
        p.extend(std::iter::repeat_n(0u8, payload));
        p
    }

    fn udp(src: [u8; 4], dst: [u8; 4], sport: u16, dport: u16) -> Vec<u8> {
        let mut p = vec![0x45, 0, 0, 0, 0, 0, 0, 0, 64, 17, 0, 0];
        p.extend_from_slice(&src);
        p.extend_from_slice(&dst);
        p.extend_from_slice(&sport.to_be_bytes());
        p.extend_from_slice(&dport.to_be_bytes());
        p.extend_from_slice(&[0, 8, 0, 0]);
        p
    }

    fn ethernet(ip: &[u8]) -> Vec<u8> {
        [&[0u8; 12][..], &[0x08, 0x00], ip].concat()
    }

    /// (초, 프레임) 목록으로 pcap을 만든다.
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

    #[test]
    fn flows_by_peer_and_service() {
        let web = [93, 184, 216, 34];
        let dns = [8, 8, 8, 8];
        let bad = [10, 0, 0, 99];
        let frames = vec![
            (100, ethernet(&udp(ME, dns, 50000, 53))),
            (100, ethernet(&udp(dns, ME, 53, 50000))),
            (101, ethernet(&tcp(ME, web, 50001, 443, 1, 0x18, 100))),
            (101, ethernet(&tcp(web, ME, 443, 50001, 9, 0x18, 1000))),
            (102, ethernet(&tcp(ME, web, 50001, 443, 101, 0x10, 0))),
            // 같은 조각을 두 번 보냄(재전송), 그리고 거부(RST)
            (103, ethernet(&tcp(ME, bad, 50002, 8080, 1, 0x18, 10))),
            (104, ethernet(&tcp(ME, bad, 50002, 8080, 1, 0x18, 10))),
            (104, ethernet(&tcp(bad, ME, 8080, 50002, 0, 0x14, 0))),
            // 내가 서버인 경우: 상대가 내 22번 포트로 들어온다.
            (105, ethernet(&tcp([192, 168, 0, 7], ME, 51000, 22, 1, 0x02, 0))),
            // 나와 상관없는 패킷, IP가 아닌 프레임
            (105, ethernet(&udp([1, 1, 1, 1], [2, 2, 2, 2], 1, 2))),
            (105, vec![0u8; 14]),
        ];
        let h = highway(&pcap(1, &frames)).unwrap();
        assert_eq!((h.me.as_str(), h.packets, h.seconds, h.more), ("192.168.0.10", 10, 5.0, 0));
        let find = |peer: &str| h.flows.iter().find(|f| f.peer == peer).unwrap();
        let w = find("93.184.216.34");
        assert_eq!((w.port, w.proto, w.class, w.sent, w.received, w.trouble), (443, "tcp", "secure", 2, 1, 0));
        assert!(w.kbps > 0.0);
        assert_eq!((find("8.8.8.8").class, find("8.8.8.8").port), ("dns", 53));
        let b = find("10.0.0.99");
        assert_eq!((b.class, b.trouble, b.sent, b.received), ("trouble", 2, 2, 1));
        assert_eq!((find("192.168.0.7").port, find("192.168.0.7").class), (22, "secure"));
        // 패킷이 많은 상대가 앞에 온다.
        assert_eq!(h.flows[0].peer, "10.0.0.99");
    }

    #[test]
    fn other_link_types_and_ipv6() {
        let ip = udp(ME, [8, 8, 8, 8], 50000, 53);
        // 리눅스 any(SLL), SLL2, IP만, VLAN
        let sll = [&[0u8; 14][..], &[0x08, 0x00], &ip].concat();
        let sll2 = [&[0x08, 0x00][..], &[0u8; 18], &ip].concat();
        let vlan = [&[0u8; 12][..], &[0x81, 0x00, 0, 5, 0x08, 0x00], &ip].concat();
        for (link, frame) in [(113, sll), (276, sll2), (101, ip.clone()), (1, vlan)] {
            assert_eq!(highway(&pcap(link, &[(1, frame)])).unwrap().flows[0].port, 53, "{link}");
        }
        let mut v6 = vec![0x60, 0, 0, 0, 0, 8, 17, 64];
        v6.extend_from_slice(&Ipv6Addr::LOCALHOST.octets());
        v6.extend_from_slice(&"2001:db8::1".parse::<Ipv6Addr>().unwrap().octets());
        v6.extend_from_slice(&[0xc3, 0x50, 0, 53, 0, 8, 0, 0]);
        let h = highway(&pcap(101, &[(1, v6)])).unwrap();
        assert_eq!((h.flows[0].peer.as_str(), h.flows[0].class), ("2001:db8::1", "dns"));
    }

    #[test]
    fn broken_files_never_panic() {
        assert!(highway(b"").is_none());
        assert!(highway(b"not a pcap file at all.....").is_none());
        assert!(highway(&pcap(1, &[])).is_none());
        assert!(highway(&pcap(999, &[(1, vec![1, 2, 3])])).is_none());
        let good = pcap(1, &[(1, ethernet(&tcp(ME, [1, 1, 1, 1], 5, 443, 1, 0x18, 5))), (2, ethernet(&udp(ME, [8, 8, 8, 8], 5, 53)))]);
        for cut in 0..good.len() {
            let _ = highway(&good[..cut]);
        }
        let mut seed = 0x1234_5678u32;
        for _ in 0..3000 {
            let mut p = good.clone();
            for _ in 0..3 {
                seed ^= seed << 13;
                seed ^= seed >> 17;
                seed ^= seed << 5;
                let i = 4 + seed as usize % (p.len() - 4);
                p[i] = (seed >> 8) as u8;
            }
            let _ = highway(&p);
        }
        // 나노초 시각, 큰 쪽이 앞인 바이트 순서
        let mut nanos = good.clone();
        nanos[..4].copy_from_slice(&[0x4d, 0x3c, 0xb2, 0xa1]);
        assert!(highway(&nanos).is_some());
    }

    #[test]
    fn capture_file_name() {
        let s = |v: &[&str]| v.iter().map(|x| x.to_string()).collect::<Vec<_>>();
        assert_eq!(capture_name(&s(&["-i", "eth0", "-w", "capture-1.pcap", "tcp"])), Some("capture-1.pcap"));
        assert_eq!(capture_name(&s(&["-w", "../x.pcap"])), None);
        assert_eq!(capture_name(&s(&["-w", "x.txt"])), None);
        assert_eq!(capture_name(&s(&["-i", "eth0"])), None);
    }
}
