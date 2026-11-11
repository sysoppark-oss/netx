//! ICMP 에코(핑) 한 번.
//!
//! Windows: `IcmpSendEcho`(iphlpapi). 관리자 권한이 필요 없다.
//! 그 밖(개발·시험용): 시스템의 `ping`을 한 번 실행해 결과를 읽는다. 배포판은 Windows뿐이라 이 길은 개발 때만 쓴다.

use std::net::Ipv4Addr;
use std::sync::Arc;

#[derive(Debug, Clone, PartialEq)]
pub enum Echo {
    /// 대상이 답함
    Reply { from: Ipv4Addr, ms: f64, ttl: u8 },
    /// 가는 길의 라우터가 TTL 만료를 알림(경로 추적에 쓴다)
    Expired { from: Ipv4Addr, ms: f64 },
    Unreachable(Option<Ipv4Addr>),
    Timeout,
    Failed(String),
}

/// (대상, TTL, 기다릴 시간 ms, 데이터 크기) → 결과. 시험에서는 가짜로 바꿔 끼운다.
pub type Prober = Arc<dyn Fn(Ipv4Addr, u8, u32, u16) -> Echo + Send + Sync>;

pub fn system() -> Prober {
    Arc::new(echo)
}

/// 기다리는 동안 다른 일을 막지 않도록 따로 도는 스레드에서 보낸다.
pub async fn probe(prober: &Prober, ip: Ipv4Addr, ttl: u8, timeout_ms: u32, size: u16) -> Echo {
    let prober = prober.clone();
    tauri::async_runtime::spawn_blocking(move || prober(ip, ttl, timeout_ms, size)).await.unwrap_or_else(|e| Echo::Failed(e.to_string()))
}

#[cfg(target_os = "windows")]
fn echo(ip: Ipv4Addr, ttl: u8, timeout_ms: u32, size: u16) -> Echo {
    use windows::Win32::Foundation::GetLastError;
    use windows::Win32::NetworkManagement::IpHelper::{ICMP_ECHO_REPLY, IP_OPTION_INFORMATION, IcmpCloseHandle, IcmpCreateFile, IcmpSendEcho};

    const IP_SUCCESS: u32 = 0;
    const IP_DEST_NET_UNREACHABLE: u32 = 11002;
    const IP_DEST_PORT_UNREACHABLE: u32 = 11005;
    const IP_REQ_TIMED_OUT: u32 = 11010;
    const IP_TTL_EXPIRED_TRANSIT: u32 = 11013;

    let size = size.min(1400);
    let data = vec![0x61u8; size as usize];
    // 답 버퍼: 구조체 + 보낸 데이터 + 여유. u64로 잡아 정렬을 맞춘다.
    let bytes = std::mem::size_of::<ICMP_ECHO_REPLY>() + size as usize + 64;
    let mut buffer = vec![0u64; bytes.div_ceil(8)];
    let options = IP_OPTION_INFORMATION { Ttl: ttl.max(1), Tos: 0, Flags: 0, OptionsSize: 0, OptionsData: std::ptr::null_mut() };
    let started = std::time::Instant::now();

    // SAFETY: 핸들은 이 함수 안에서 열고 닫는다. 데이터와 답 버퍼는 호출이 끝날 때까지 살아 있고,
    // 넘기는 크기는 실제로 잡은 크기다. 답이 하나 이상일 때만(count > 0) 버퍼 앞의 구조체를 읽는다.
    unsafe {
        let handle = match IcmpCreateFile() {
            Ok(h) => h,
            Err(e) => return Echo::Failed(format!("ICMP를 열지 못했습니다: {e}")),
        };
        let count = IcmpSendEcho(handle, u32::from_ne_bytes(ip.octets()), data.as_ptr().cast(), size, Some(&options as *const _), buffer.as_mut_ptr().cast(), (buffer.len() * 8) as u32, timeout_ms.max(1));
        let error = GetLastError().0;
        let ms = started.elapsed().as_secs_f64() * 1000.0;
        let _ = IcmpCloseHandle(handle);
        if count == 0 {
            return match error {
                IP_REQ_TIMED_OUT => Echo::Timeout,
                IP_DEST_NET_UNREACHABLE..=IP_DEST_PORT_UNREACHABLE => Echo::Unreachable(None),
                other => Echo::Failed(format!("ICMP 오류 {other}")),
            };
        }
        let reply = &*(buffer.as_ptr() as *const ICMP_ECHO_REPLY);
        let from = Ipv4Addr::from(reply.Address.to_ne_bytes());
        match reply.Status {
            IP_SUCCESS => Echo::Reply { from, ms, ttl: reply.Options.Ttl },
            IP_TTL_EXPIRED_TRANSIT => Echo::Expired { from, ms },
            IP_DEST_NET_UNREACHABLE..=IP_DEST_PORT_UNREACHABLE => Echo::Unreachable(Some(from)),
            IP_REQ_TIMED_OUT => Echo::Timeout,
            other => Echo::Failed(format!("ICMP 상태 {other}")),
        }
    }
}

#[cfg(not(target_os = "windows"))]
fn echo(ip: Ipv4Addr, ttl: u8, timeout_ms: u32, size: u16) -> Echo {
    let wait = timeout_ms.div_ceil(1000).max(1).to_string();
    let output = std::process::Command::new("ping")
        .args(["-n", "-c", "1", "-W", &wait, "-t", &ttl.max(1).to_string(), "-s", &size.min(1400).to_string(), &ip.to_string()])
        .stdin(std::process::Stdio::null())
        .output();
    match output {
        Ok(out) => parse_linux(&String::from_utf8_lossy(&out.stdout)),
        Err(e) => Echo::Failed(format!("ping을 실행하지 못했습니다: {e}")),
    }
}

/// 리눅스 `ping -c 1`의 출력에서 결과를 읽는다(개발·시험용).
#[cfg(any(test, not(target_os = "windows")))]
fn parse_linux(text: &str) -> Echo {
    let after = |line: &str, key: &str| line.split_once(key).and_then(|(_, r)| r.split([' ', ':']).next().map(str::to_string));
    for line in text.lines() {
        if line.contains("bytes from") {
            let from = after(line, "from ").and_then(|s| s.parse().ok());
            let ms = after(line, "time=").and_then(|s| s.parse().ok());
            let ttl = after(line, "ttl=").and_then(|s| s.parse().ok());
            if let (Some(from), Some(ms), Some(ttl)) = (from, ms, ttl) {
                return Echo::Reply { from, ms, ttl };
            }
        } else if let Some(from) = line.strip_prefix("From ").and_then(|r| r.split(' ').next()).and_then(|s| s.parse().ok()) {
            return if line.contains("Time to live exceeded") { Echo::Expired { from, ms: 0.0 } } else { Echo::Unreachable(Some(from)) };
        }
    }
    Echo::Timeout
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_linux_ping_output() {
        assert_eq!(parse_linux("PING 8.8.8.8 (8.8.8.8) 56(84) bytes of data.\n64 bytes from 8.8.8.8: icmp_seq=1 ttl=115 time=31.2 ms\n"), Echo::Reply { from: Ipv4Addr::new(8, 8, 8, 8), ms: 31.2, ttl: 115 });
        assert_eq!(parse_linux("From 10.0.0.1 icmp_seq=1 Time to live exceeded\n"), Echo::Expired { from: Ipv4Addr::new(10, 0, 0, 1), ms: 0.0 });
        assert_eq!(parse_linux("From 10.0.0.1 icmp_seq=1 Destination Host Unreachable\n"), Echo::Unreachable(Some(Ipv4Addr::new(10, 0, 0, 1))));
        assert_eq!(parse_linux("PING x\n\n--- x ping statistics ---\n1 packets transmitted, 0 received\n"), Echo::Timeout);
        assert_eq!(parse_linux(""), Echo::Timeout);
    }

    /// 실제 ICMP: 자기 자신은 항상 답한다. Windows 빌드(Actions)에서 Windows 쪽 코드를 확인한다.
    #[cfg(target_os = "windows")]
    #[test]
    fn loopback_answers() {
        match echo(Ipv4Addr::LOCALHOST, 64, 2000, 32) {
            Echo::Reply { from, ms, .. } => assert!(from == Ipv4Addr::LOCALHOST && (0.0..2000.0).contains(&ms)),
            other => panic!("{other:?}"),
        }
        // 쓰지 않는 시험용 대역(TEST-NET-1)은 답이 없다.
        assert!(!matches!(echo(Ipv4Addr::new(192, 0, 2, 1), 64, 300, 32), Echo::Reply { .. }));
    }
}
