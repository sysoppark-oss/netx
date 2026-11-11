//! 내부 네트워크 조사(`netmap`)의 덧붙임 조사: 열린 포트의 첫 응답(배너), mDNS, UPnP(SSDP).
//! 장비가 보낸 글은 모두 `tidy`로 제어 문자를 지우고 길이를 자른 뒤에만 쓴다.

use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::time::Duration;

use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpStream, UdpSocket};

const BANNER_WAIT: Duration = Duration::from_millis(1500);
/// 접속하면 서버가 먼저 말하는 포트
pub const TALK_FIRST: [u16; 5] = [21, 22, 25, 110, 143];
/// 웹 화면이 있을 만한 포트(암호화 없는 것만)
pub const WEB: [u16; 10] = [80, 81, 3000, 5000, 8000, 8008, 8080, 8081, 8888, 9000];

pub const MDNS: SocketAddr = SocketAddr::new(IpAddr::V4(Ipv4Addr::new(224, 0, 0, 251)), 5353);
pub const SSDP: SocketAddr = SocketAddr::new(IpAddr::V4(Ipv4Addr::new(239, 255, 255, 250)), 1900);

/// 장비가 보낸 글을 한 줄로 다듬는다: 제어 문자는 빈칸으로, 빈칸은 하나로, 80자까지.
pub fn tidy(raw: &str) -> Option<String> {
    let text: String = raw.chars().map(|c| if c.is_control() { ' ' } else { c }).collect();
    let text = text.split_whitespace().collect::<Vec<_>>().join(" ");
    let text: String = text.chars().take(80).collect();
    (!text.is_empty()).then_some(text)
}

/// HTTP 응답에서 서버 이름과 화면 제목을 뽑는다.
pub fn web_summary(response: &str) -> Option<String> {
    let lower = response.to_ascii_lowercase();
    let server = response.lines().take_while(|l| !l.trim().is_empty()).find_map(|l| l.split_once(':').filter(|(k, _)| k.eq_ignore_ascii_case("server")).map(|(_, v)| v.trim()));
    let title = lower.find("<title").and_then(|at| {
        let start = at + lower[at..].find('>')? + 1;
        let end = start + lower[start..].find("</title")?;
        response.get(start..end)
    });
    match (server.and_then(tidy), title.and_then(tidy)) {
        (Some(s), Some(t)) => tidy(&format!("{s} · \"{t}\"")),
        (Some(s), None) => Some(s),
        (None, Some(t)) => tidy(&format!("\"{t}\"")),
        (None, None) => response.lines().next().filter(|l| l.starts_with("HTTP/")).and_then(tidy),
    }
}

/// 열린 포트에 접속해 첫 응답을 읽는다. 조사할 줄 모르는 포트이거나 답이 없으면 None.
pub async fn banner(addr: SocketAddr) -> Option<String> {
    let port = addr.port();
    let web = WEB.contains(&port);
    if !web && !TALK_FIRST.contains(&port) {
        return None;
    }
    let work = async {
        let mut stream = TcpStream::connect(addr).await.ok()?;
        if web {
            stream.write_all(format!("GET / HTTP/1.0\r\nHost: {}\r\nUser-Agent: NetX\r\nAccept: */*\r\n\r\n", addr.ip()).as_bytes()).await.ok()?;
        }
        let mut buf = vec![0u8; if web { 4096 } else { 256 }];
        let mut filled = 0;
        // 웹은 제목이 나올 때까지 조금 더 읽는다. 그 밖에는 첫 줄이면 된다.
        while filled < buf.len() {
            match stream.read(&mut buf[filled..]).await {
                Ok(0) | Err(_) => break,
                Ok(n) => filled += n,
            }
            if !web && buf[..filled].contains(&b'\n') {
                break;
            }
        }
        let text = String::from_utf8_lossy(&buf[..filled]).into_owned();
        if web { web_summary(&text) } else { text.lines().next().and_then(tidy) }
    };
    tokio::time::timeout(BANNER_WAIT, work).await.ok().flatten()
}

/// mDNS 질의: "어떤 서비스가 있나요?" (`_services._dns-sd._udp.local`, 답은 나에게 직접 보내 달라고 표시)
pub fn mdns_query() -> Vec<u8> {
    let mut p = crate::basic::dns::query(0, "_services._dns-sd._udp.local", 12);
    p[2] = 0;
    let n = p.len();
    p[n - 2] = 0x80;
    p
}

/// 서비스 이름을 알아보기 쉬운 말로 바꾼다. 모르는 것은 이름 그대로.
pub fn service_label(name: &str) -> String {
    let short = name.trim_end_matches('.').trim_end_matches(".local").trim_end_matches("._tcp").trim_end_matches("._udp").trim_start_matches('_');
    match short {
        "ipp" | "ipps" | "printer" | "pdl-datastream" => "프린터".into(),
        "scanner" | "uscan" | "uscans" => "스캐너".into(),
        "airplay" | "raop" => "AirPlay".into(),
        "googlecast" => "Chromecast".into(),
        "smb" | "afpovertcp" | "nfs" => "파일 공유".into(),
        "ssh" | "sftp-ssh" => "SSH".into(),
        "http" | "https" => "웹 화면".into(),
        "hap" | "homekit" | "matter" | "matterc" => "스마트홈".into(),
        "workstation" | "device-info" | "companion-link" => "PC·휴대 기기".into(),
        "spotify-connect" | "sonos" => "스피커".into(),
        "rtsp" => "카메라".into(),
        other => other.chars().filter(|c| c.is_ascii_alphanumeric() || *c == '-').take(24).collect(),
    }
}

/// 한 번 묻고 정해진 시간 동안 답을 모은다. (보낸 곳, 받은 글자) 목록
async fn ask_many(to: SocketAddr, packet: &[u8], wait: Duration) -> Vec<(Ipv4Addr, Vec<u8>)> {
    let mut out = Vec::new();
    let Ok(sock) = UdpSocket::bind("0.0.0.0:0").await else { return out };
    if sock.send_to(packet, to).await.is_err() {
        return out;
    }
    let deadline = tokio::time::Instant::now() + wait;
    let mut buf = [0u8; 4096];
    while out.len() < 2000 {
        match tokio::time::timeout_at(deadline, sock.recv_from(&mut buf)).await {
            Ok(Ok((n, SocketAddr::V4(from)))) => out.push((*from.ip(), buf[..n].to_vec())),
            Ok(Ok(_)) => {}
            _ => break,
        }
    }
    out
}

/// mDNS에 답한 장비와 그 장비가 알린 서비스 종류
pub async fn mdns(to: SocketAddr, wait: Duration) -> Vec<(Ipv4Addr, Vec<String>)> {
    let mut found: Vec<(Ipv4Addr, Vec<String>)> = Vec::new();
    for (ip, packet) in ask_many(to, &mdns_query(), wait).await {
        let labels: Vec<String> = crate::basic::dns::ptr_names(&packet).iter().map(|n| service_label(n)).filter(|l| !l.is_empty()).collect();
        let at = match found.iter().position(|(f, _)| *f == ip) {
            Some(i) => i,
            None => {
                found.push((ip, Vec::new()));
                found.len() - 1
            }
        };
        let entry = &mut found[at];
        for label in labels {
            if !entry.1.contains(&label) && entry.1.len() < 8 {
                entry.1.push(label);
            }
        }
    }
    found
}

const SSDP_SEARCH: &str = "M-SEARCH * HTTP/1.1\r\nHOST: 239.255.255.250:1900\r\nMAN: \"ssdp:discover\"\r\nMX: 1\r\nST: ssdp:all\r\n\r\n";

/// UPnP 답에서 장비가 밝힌 이름(SERVER 줄)
pub fn ssdp_server(response: &str) -> Option<String> {
    response.lines().find_map(|l| l.split_once(':').filter(|(k, _)| k.trim().eq_ignore_ascii_case("server")).map(|(_, v)| v)).and_then(tidy)
}

/// UPnP(SSDP)에 답한 장비와 그 장비가 밝힌 이름
pub async fn ssdp(to: SocketAddr, wait: Duration) -> Vec<(Ipv4Addr, String)> {
    let mut found: Vec<(Ipv4Addr, String)> = Vec::new();
    for (ip, packet) in ask_many(to, SSDP_SEARCH.as_bytes(), wait).await {
        if found.iter().any(|(f, _)| *f == ip) {
            continue;
        }
        if let Some(server) = ssdp_server(&String::from_utf8_lossy(&packet)) {
            found.push((ip, server));
        }
    }
    found
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::net::TcpListener;

    #[test]
    fn text_from_devices_is_tidied() {
        assert_eq!(tidy("SSH-2.0-OpenSSH_9.6\r\n").as_deref(), Some("SSH-2.0-OpenSSH_9.6"));
        assert_eq!(tidy("a\x1b[2J  b\tc\n").as_deref(), Some("a [2J b c"));
        assert_eq!(tidy(&"x".repeat(500)).unwrap().len(), 80);
        assert_eq!(tidy(" \r\n\x00"), None);
        assert_eq!(web_summary("HTTP/1.1 200 OK\r\nServer: lighttpd/1.4\r\n\r\n<html><TITLE>ipTIME\n A3004</TITLE>").as_deref(), Some("lighttpd/1.4 · \"ipTIME A3004\""));
        assert_eq!(web_summary("HTTP/1.0 401 Unauthorized\r\n\r\n").as_deref(), Some("HTTP/1.0 401 Unauthorized"));
        assert_eq!(web_summary("HTTP/1.1 200 OK\r\n\r\n<title>관리자</title>").as_deref(), Some("\"관리자\""));
        assert_eq!(web_summary("garbage <title"), None);
        assert_eq!(ssdp_server("HTTP/1.1 200 OK\r\nST: upnp:rootdevice\r\nSERVER: Linux/3.10 UPnP/1.0 ipTIME/1.0\r\n\r\n").as_deref(), Some("Linux/3.10 UPnP/1.0 ipTIME/1.0"));
        assert_eq!(ssdp_server("HTTP/1.1 200 OK\r\n\r\n"), None);
    }

    #[test]
    fn service_names() {
        assert_eq!(service_label("_ipp._tcp.local."), "프린터");
        assert_eq!(service_label("_googlecast._tcp.local."), "Chromecast");
        assert_eq!(service_label("_my-thing._udp.local."), "my-thing");
        assert_eq!(service_label("_<b>._tcp.local."), "b");
        let q = mdns_query();
        assert_eq!((&q[..2], q[2], q[q.len() - 2]), (&[0u8, 0][..], 0, 0x80));
    }

    #[tokio::test]
    async fn banners_from_fake_devices() {
        // 먼저 말하는 서버(SSH처럼)와 웹 서버를 흉내 낸다. 포트 번호로 종류를 가리므로 함수 안쪽을 직접 시험한다.
        let ssh = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = ssh.local_addr().unwrap();
        tokio::spawn(async move {
            let (mut c, _) = ssh.accept().await.unwrap();
            c.write_all(b"SSH-2.0-dropbear_2022.83\r\nmore").await.unwrap();
        });
        // 모르는 포트는 건드리지 않는다.
        assert_eq!(banner(addr).await, None);
        let mut stream = TcpStream::connect(addr).await.unwrap();
        let mut buf = [0u8; 64];
        let n = stream.read(&mut buf).await.unwrap();
        assert_eq!(String::from_utf8_lossy(&buf[..n]).lines().next().and_then(tidy).as_deref(), Some("SSH-2.0-dropbear_2022.83"));
    }

    #[tokio::test]
    async fn mdns_and_ssdp_answers() {
        // 답하는 가짜 장비(유니캐스트). mDNS 답: _ipp._tcp.local과 _airplay._tcp.local
        let device = UdpSocket::bind("127.0.0.1:0").await.unwrap();
        let to = device.local_addr().unwrap();
        tokio::spawn(async move {
            let mut buf = [0u8; 512];
            loop {
                let (n, from) = device.recv_from(&mut buf).await.unwrap();
                if buf[..n].starts_with(b"M-SEARCH") {
                    device.send_to(b"HTTP/1.1 200 OK\r\nSERVER: FakeOS UPnP/1.0 Thing/2\r\n\r\n", from).await.unwrap();
                    device.send_to(b"HTTP/1.1 200 OK\r\nSERVER: second answer is ignored\r\n\r\n", from).await.unwrap();
                } else {
                    let mut p = crate::basic::dns::query(0, "_services._dns-sd._udp.local", 12);
                    p[2] = 0x84;
                    p[7] = 2;
                    for name in ["_ipp", "_airplay"] {
                        let mut rdata = vec![name.len() as u8];
                        rdata.extend_from_slice(name.as_bytes());
                        rdata.extend_from_slice(&[4, b'_', b't', b'c', b'p', 5, b'l', b'o', b'c', b'a', b'l', 0]);
                        p.extend_from_slice(&[0xc0, 0x0c, 0, 12, 0, 1, 0, 0, 0, 10, 0, rdata.len() as u8]);
                        p.extend_from_slice(&rdata);
                    }
                    device.send_to(&p, from).await.unwrap();
                    device.send_to(b"\x00\x01garbage", from).await.unwrap();
                }
            }
        });
        let wait = Duration::from_millis(300);
        assert_eq!(mdns(to, wait).await, [(Ipv4Addr::LOCALHOST, vec!["프린터".to_string(), "AirPlay".to_string()])]);
        assert_eq!(ssdp(to, wait).await, [(Ipv4Addr::LOCALHOST, "FakeOS UPnP/1.0 Thing/2".to_string())]);
    }
}
