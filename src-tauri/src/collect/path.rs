//! 거점 지도의 경로: 이 PC의 테일스케일에게 "거점까지 어떤 길로 가는가"를 읽기만 묻는다.
//! `tailscale status --json`을 인자 배열로 실행한다(셸 없음, 인자 고정, 사용자 입력 없음). 관리 콘솔 API나 토큰은 쓰지 않는다.
//! 답은 밖에서 온 자료로 다룬다: `get`으로만 읽고, 글자는 허용한 것만 남기고, 어떤 입력에도 죽지 않는다.

use std::collections::HashMap;
use std::net::Ipv4Addr;

use serde::Serialize;
use serde_json::Value;

/// 답의 크기 상한(큰 테일넷이라도 이 안에 든다. 넘으면 읽지 않는다)
const MAX_OUTPUT: usize = 4 * 1024 * 1024;
const MAX_PEERS: usize = 2000;

/// 한 거점까지의 길
#[derive(Debug, Clone, Default, Serialize, PartialEq)]
pub struct Path {
    /// direct(직접), relay(중계 서버를 거침), idle(지금 주고받지 않음)
    pub kind: &'static str,
    /// 직접일 때 상대의 주소:포트
    pub addr: String,
    /// 중계 서버의 지역 이름(예: tok)
    pub relay: String,
    pub online: bool,
    pub rx: u64,
    pub tx: u64,
}

fn clean(text: &str, max: usize) -> String {
    text.chars().filter(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | ':' | '-' | '[' | ']')).take(max).collect()
}

/// 테일스케일의 답에서 상대 주소(100.x) → 길. 모양이 아니면 빈 표.
pub fn parse(status: &Value) -> HashMap<Ipv4Addr, Path> {
    let mut out = HashMap::new();
    let Some(peers) = status.get("Peer").and_then(Value::as_object) else { return out };
    for peer in peers.values().take(MAX_PEERS) {
        let Some(ip) = peer.get("TailscaleIPs").and_then(Value::as_array).and_then(|a| a.iter().filter_map(Value::as_str).find_map(|s| s.parse::<Ipv4Addr>().ok())) else { continue };
        if !super::tailnet(ip) {
            continue;
        }
        let text = |k: &str| peer.get(k).and_then(Value::as_str).unwrap_or("");
        let (addr, relay) = (clean(text("CurAddr"), 60), clean(text("Relay"), 16));
        let active = peer.get("Active").and_then(Value::as_bool).unwrap_or(false);
        let kind = if !addr.is_empty() {
            "direct"
        } else if active && !relay.is_empty() {
            "relay"
        } else {
            "idle"
        };
        let num = |k: &str| peer.get(k).and_then(Value::as_u64).unwrap_or(0);
        out.insert(ip, Path { kind, addr, relay, online: peer.get("Online").and_then(Value::as_bool).unwrap_or(false), rx: num("RxBytes"), tx: num("TxBytes") });
    }
    out
}

/// 테일스케일 프로그램. Windows에서는 Program Files의 정해진 자리만 본다(PATH를 따르지 않는다).
fn program() -> Option<std::path::PathBuf> {
    #[cfg(windows)]
    {
        let root = std::env::var_os("ProgramFiles").map(std::path::PathBuf::from).filter(|p| p.is_absolute())?;
        let exe = root.join("Tailscale").join("tailscale.exe");
        exe.is_file().then_some(exe)
    }
    #[cfg(not(windows))]
    {
        ["/usr/bin/tailscale", "/usr/local/bin/tailscale"].into_iter().map(std::path::PathBuf::from).find(|p| p.is_file())
    }
}

/// 지금의 길 표. 테일스케일이 없거나 답하지 않으면 `None`(지도는 "경로 모름"으로 그린다).
pub fn read() -> Option<HashMap<Ipv4Addr, Path>> {
    let mut cmd = std::process::Command::new(program()?);
    cmd.args(["status", "--json"]).stdin(std::process::Stdio::null()).stderr(std::process::Stdio::null());
    #[cfg(windows)]
    std::os::windows::process::CommandExt::creation_flags(&mut cmd, 0x0800_0000);
    let out = cmd.output().ok()?;
    if !out.status.success() || out.stdout.len() > MAX_OUTPUT {
        return None;
    }
    Some(parse(&serde_json::from_slice(&out.stdout).ok()?))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn reads_direct_relayed_and_idle_peers() {
        let v = json!({"Peer": {
            "nodekey:a": {"TailscaleIPs": ["100.99.126.111", "fd7a:115c:a1e0::1"], "CurAddr": "121.1.2.3:41641", "Relay": "tok", "Active": true, "Online": true, "RxBytes": 10, "TxBytes": 20},
            "nodekey:b": {"TailscaleIPs": ["100.64.0.9"], "CurAddr": "", "Relay": "tok", "Active": true, "Online": true},
            "nodekey:c": {"TailscaleIPs": ["100.64.0.10"], "CurAddr": "", "Relay": "sel", "Active": false, "Online": false},
            "nodekey:d": {"TailscaleIPs": ["8.8.8.8"], "CurAddr": "1.1.1.1:1"},
        }});
        let p = parse(&v);
        assert_eq!(p.len(), 3, "테일스케일 대역이 아닌 주소는 버린다");
        let a = &p[&"100.99.126.111".parse::<Ipv4Addr>().unwrap()];
        assert_eq!((a.kind, a.addr.as_str(), a.rx, a.tx, a.online), ("direct", "121.1.2.3:41641", 10, 20, true));
        assert_eq!(p[&"100.64.0.9".parse::<Ipv4Addr>().unwrap()].kind, "relay");
        assert_eq!(p[&"100.64.0.10".parse::<Ipv4Addr>().unwrap()].kind, "idle");
    }

    #[test]
    fn strange_answers_never_panic_and_text_is_cleaned() {
        for v in [json!(null), json!([]), json!({"Peer": 3}), json!({"Peer": {"x": null}}), json!({"Peer": {"x": {"TailscaleIPs": "100.64.0.1"}}}), json!({"Peer": {"x": {"TailscaleIPs": [1, {}, "nope"]}}})] {
            assert!(parse(&v).is_empty());
        }
        let v = json!({"Peer": {"x": {"TailscaleIPs": ["100.64.0.2"], "CurAddr": "1.2.3.4:5<script>\u{202E}", "Relay": "to k;rm", "RxBytes": -5, "Active": "yes"}}});
        let p = &parse(&v)[&"100.64.0.2".parse::<Ipv4Addr>().unwrap()];
        assert_eq!((p.addr.as_str(), p.relay.as_str(), p.rx), ("1.2.3.4:5script", "tokrm", 0));
        // 무작위 바이트를 JSON으로 읽을 수 있을 때도 죽지 않는다
        let mut seed = 7u32;
        for _ in 0..300 {
            let bytes: Vec<u8> = (0..40)
                .map(|_| {
                    seed = seed.wrapping_mul(1_103_515_245).wrapping_add(12_345);
                    b"{}[]\":,0123456789truefalsnPeerTailscaleIPs. "[(seed >> 16) as usize % 44]
                })
                .collect();
            if let Ok(v) = serde_json::from_slice::<Value>(&bytes) {
                let _ = parse(&v);
            }
        }
    }
}
