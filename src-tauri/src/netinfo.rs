//! 내 네트워크 정보: 쓰고 있는 어댑터의 주소, 게이트웨이, DNS, 공인 IP, 주고받은 양.
//!
//! 조회 명령은 코드에 고정되어 있고 사용자 입력이 들어가지 않는다.
//! 공인 IP는 밖에서 본 내 주소라서 외부 서비스(api.ipify.org)에 물어야 알 수 있다. 주소 말고는 아무것도 보내지 않는다.

use std::net::IpAddr;
use std::process::Stdio;
use std::sync::Mutex;
use std::time::{Duration, Instant};

use serde::Serialize;
use serde_json::Value;
use tokio::process::Command;

use crate::env_check::hide_console_window;

#[derive(Debug, Clone, Default, Serialize)]
pub struct NetInfo {
    pub iface: Option<String>,
    pub ip: Option<String>,
    pub prefix: Option<u8>,
    pub gateway: Option<String>,
    pub dns: Vec<String>,
    pub link: Option<String>,
    pub public_ip: Option<String>,
    /// 어댑터가 켜진 뒤로 받은·보낸 바이트. 화면이 두 번의 차이로 속도를 계산한다.
    pub rx_bytes: Option<u64>,
    pub tx_bytes: Option<u64>,
    /// 측정 시각(밀리초). 속도 계산용.
    pub at_ms: u64,
}

#[derive(Clone, Default)]
struct Config {
    iface: Option<String>,
    ip: Option<String>,
    prefix: Option<u8>,
    gateway: Option<String>,
    dns: Vec<String>,
    link: Option<String>,
}

#[derive(Default)]
struct Cache {
    config: Option<(Instant, Config)>,
    public: Option<(Instant, Option<String>)>,
}

static CACHE: Mutex<Option<Cache>> = Mutex::new(None);
static COUNTERS: Mutex<Option<sysinfo::Networks>> = Mutex::new(None);
static START: Mutex<Option<Instant>> = Mutex::new(None);

const CONFIG_TTL: Duration = Duration::from_secs(20);
const PUBLIC_TTL: Duration = Duration::from_secs(600);
const PUBLIC_RETRY: Duration = Duration::from_secs(60);

fn cache<R>(f: impl FnOnce(&mut Cache) -> R) -> R {
    let mut g = CACHE.lock().unwrap_or_else(|e| e.into_inner());
    f(g.get_or_insert_with(Cache::default))
}

pub async fn current() -> NetInfo {
    let config = match cache(|c| c.config.clone().filter(|(at, _)| at.elapsed() < CONFIG_TTL)) {
        Some((_, cfg)) => cfg,
        None => {
            let fresh = read_config().await;
            cache(|c| {
                let cfg = keep_last_good(c.config.take().map(|(_, old)| old), fresh);
                c.config = Some((Instant::now(), cfg.clone()));
                cfg
            })
        }
    };

    let fresh = cache(|c| c.public.clone()).filter(|(at, ip)| at.elapsed() < if ip.is_some() { PUBLIC_TTL } else { PUBLIC_RETRY });
    let public_ip = match fresh {
        Some((_, ip)) => ip,
        None => {
            // 여러 번 겹쳐 묻지 않게 먼저 자리를 잡아 둔다.
            let previous = cache(|c| c.public.replace((Instant::now(), c.public.as_ref().and_then(|(_, ip)| ip.clone())))).and_then(|(_, ip)| ip);
            let ip = fetch_public_ip().await.or(previous);
            cache(|c| c.public = Some((Instant::now(), ip.clone())));
            ip
        }
    };

    let (rx_bytes, tx_bytes) = counters(config.iface.as_deref());
    let at_ms = {
        let mut g = START.lock().unwrap_or_else(|e| e.into_inner());
        g.get_or_insert_with(Instant::now).elapsed().as_millis() as u64
    };
    NetInfo {
        iface: config.iface,
        ip: config.ip,
        prefix: config.prefix,
        gateway: config.gateway,
        dns: config.dns,
        link: config.link,
        public_ip,
        rx_bytes,
        tx_bytes,
        at_ms,
    }
}

/// 조회가 잠깐 실패해 빈 값이 오면(PowerShell이 늦게 뜨는 경우 등) 직전 값을 그대로 쓴다.
fn keep_last_good(previous: Option<Config>, fresh: Config) -> Config {
    match previous {
        Some(old) if fresh.ip.is_none() && old.ip.is_some() => old,
        _ => fresh,
    }
}

/// 어댑터의 누적 송수신량. 이름이 맞는 어댑터가 없으면 가장 많이 쓴 어댑터를 고른다.
fn counters(iface: Option<&str>) -> (Option<u64>, Option<u64>) {
    let mut g = COUNTERS.lock().unwrap_or_else(|e| e.into_inner());
    let nets = match g.as_mut() {
        Some(n) => {
            n.refresh(true);
            n
        }
        None => g.insert(sysinfo::Networks::new_with_refreshed_list()),
    };
    let pick = iface
        .and_then(|name| nets.iter().find(|(n, _)| n.as_str() == name))
        .or_else(|| nets.iter().filter(|(n, _)| !is_loopback(n)).max_by_key(|(_, d)| d.total_received() + d.total_transmitted()));
    match pick {
        Some((_, d)) => (Some(d.total_received()), Some(d.total_transmitted())),
        None => (None, None),
    }
}

fn is_loopback(name: &str) -> bool {
    let n = name.to_ascii_lowercase();
    n == "lo" || n.contains("loopback")
}

async fn run(program: &str, args: &[&str]) -> Option<String> {
    let mut cmd = Command::new(program);
    cmd.args(args).stdin(Stdio::null()).stderr(Stdio::null()).kill_on_drop(true);
    hide_console_window(&mut cmd);
    match tokio::time::timeout(Duration::from_secs(25), cmd.output()).await {
        Ok(Ok(out)) if out.status.success() => Some(String::from_utf8_lossy(&out.stdout).into_owned()),
        _ => None,
    }
}

const WINDOWS_SCRIPT: &str = r#"$ErrorActionPreference = 'SilentlyContinue'
[Console]::OutputEncoding = [System.Text.Encoding]::UTF8
$c = Get-NetIPConfiguration | Where-Object { $_.IPv4DefaultGateway -ne $null -and $_.NetAdapter.Status -eq 'Up' } | Select-Object -First 1
if ($c) {
  $dns = @($c.DNSServer | Where-Object { $_.AddressFamily -eq 2 } | ForEach-Object { $_.ServerAddresses })
  [pscustomobject]@{
    iface = [string]$c.InterfaceAlias
    ip = [string]$c.IPv4Address[0].IPAddress
    prefix = [int]$c.IPv4Address[0].PrefixLength
    gateway = [string]$c.IPv4DefaultGateway[0].NextHop
    dns = ($dns -join ',')
    link = [string]$c.NetAdapter.LinkSpeed
  } | ConvertTo-Json -Compress
}
"#;

async fn read_config() -> Config {
    if cfg!(target_os = "windows") {
        let out = run("powershell.exe", &["-NoLogo", "-NoProfile", "-NonInteractive", "-Command", WINDOWS_SCRIPT]).await;
        out.as_deref().map(parse_windows).unwrap_or_default()
    } else {
        let route = run("ip", &["-j", "route", "show", "default"]).await.unwrap_or_default();
        let (iface, gateway) = parse_linux_route(&route);
        let mut cfg = Config { iface: iface.clone(), gateway, ..Config::default() };
        if let Some(dev) = iface {
            let addr = run("ip", &["-j", "-4", "addr", "show", "dev", &dev]).await.unwrap_or_default();
            (cfg.ip, cfg.prefix) = parse_linux_addr(&addr);
        }
        cfg.dns = std::fs::read_to_string("/etc/resolv.conf").map(|t| parse_resolv(&t)).unwrap_or_default();
        cfg
    }
}

fn ip_text(v: Option<&Value>) -> Option<String> {
    v?.as_str()?.trim().parse::<IpAddr>().ok().map(|ip| ip.to_string())
}

fn short_text(v: Option<&Value>) -> Option<String> {
    let s = v?.as_str()?.trim();
    (!s.is_empty()).then(|| s.chars().filter(|c| !c.is_control()).take(64).collect())
}

fn prefix_of(v: Option<&Value>) -> Option<u8> {
    v?.as_u64().filter(|p| *p <= 128).map(|p| p as u8)
}

fn parse_windows(stdout: &str) -> Config {
    let Ok(v) = serde_json::from_str::<Value>(stdout.trim().trim_start_matches('\u{feff}')) else {
        return Config::default();
    };
    Config {
        iface: short_text(v.get("iface")),
        ip: ip_text(v.get("ip")),
        prefix: prefix_of(v.get("prefix")),
        gateway: ip_text(v.get("gateway")),
        dns: v.get("dns").and_then(Value::as_str).map(parse_ip_list).unwrap_or_default(),
        link: short_text(v.get("link")),
    }
}

fn parse_ip_list(text: &str) -> Vec<String> {
    text.split([',', ' ']).filter_map(|s| s.trim().parse::<IpAddr>().ok()).map(|ip| ip.to_string()).take(4).collect()
}

fn parse_linux_route(stdout: &str) -> (Option<String>, Option<String>) {
    let v: Value = serde_json::from_str(stdout).unwrap_or(Value::Null);
    let first = v.get(0);
    (short_text(first.and_then(|r| r.get("dev"))), ip_text(first.and_then(|r| r.get("gateway"))))
}

fn parse_linux_addr(stdout: &str) -> (Option<String>, Option<u8>) {
    let v: Value = serde_json::from_str(stdout).unwrap_or(Value::Null);
    let info = v.get(0).and_then(|d| d.get("addr_info")).and_then(|a| a.get(0));
    (ip_text(info.and_then(|i| i.get("local"))), prefix_of(info.and_then(|i| i.get("prefixlen"))))
}

fn parse_resolv(text: &str) -> Vec<String> {
    text.lines()
        .filter_map(|l| l.trim().strip_prefix("nameserver"))
        .filter_map(|s| s.trim().parse::<IpAddr>().ok())
        .map(|ip| ip.to_string())
        .take(4)
        .collect()
}

async fn fetch_public_ip() -> Option<String> {
    let client = reqwest::Client::builder().timeout(Duration::from_secs(6)).build().ok()?;
    for url in ["https://api.ipify.org", "https://ifconfig.me/ip"] {
        if let Ok(resp) = client.get(url).send().await
            && resp.status().is_success()
            && let Ok(text) = resp.text().await
            && let Ok(ip) = text.trim().parse::<IpAddr>()
        {
            return Some(ip.to_string());
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_windows_json() {
        let c = parse_windows("\u{feff}{\"iface\":\"Wi-Fi\",\"ip\":\"192.168.0.12\",\"prefix\":24,\"gateway\":\"192.168.0.1\",\"dns\":\"168.126.63.1,8.8.8.8\",\"link\":\"866.7 Mbps\"}\r\n");
        assert_eq!(c.iface.as_deref(), Some("Wi-Fi"));
        assert_eq!(c.ip.as_deref(), Some("192.168.0.12"));
        assert_eq!(c.prefix, Some(24));
        assert_eq!(c.gateway.as_deref(), Some("192.168.0.1"));
        assert_eq!(c.dns, ["168.126.63.1", "8.8.8.8"]);
        assert_eq!(c.link.as_deref(), Some("866.7 Mbps"));
    }

    #[test]
    fn bad_output_becomes_empty_not_garbage() {
        let c = parse_windows("{\"iface\":\"x\",\"ip\":\"<script>\",\"prefix\":999,\"gateway\":\"1.2.3\",\"dns\":\"a,b\"}");
        assert!(c.ip.is_none() && c.prefix.is_none() && c.gateway.is_none() && c.dns.is_empty());
        assert!(parse_windows("오류").iface.is_none());
        assert!(parse_windows("").iface.is_none());
    }

    #[test]
    fn a_failed_refresh_keeps_the_last_good_values() {
        let good = Config { iface: Some("Wi-Fi".into()), ip: Some("192.168.0.2".into()), ..Config::default() };
        assert_eq!(keep_last_good(Some(good.clone()), Config::default()).ip.as_deref(), Some("192.168.0.2"));
        let moved = Config { ip: Some("10.0.0.9".into()), ..Config::default() };
        assert_eq!(keep_last_good(Some(good), moved).ip.as_deref(), Some("10.0.0.9"));
        assert!(keep_last_good(None, Config::default()).ip.is_none());
    }

    #[test]
    fn parses_linux_outputs() {
        let (dev, gw) = parse_linux_route(r#"[{"dst":"default","gateway":"172.20.0.1","dev":"eth0","flags":[]}]"#);
        assert_eq!((dev.as_deref(), gw.as_deref()), (Some("eth0"), Some("172.20.0.1")));
        let (ip, prefix) = parse_linux_addr(r#"[{"ifname":"eth0","addr_info":[{"family":"inet","local":"172.20.5.9","prefixlen":20}]}]"#);
        assert_eq!((ip.as_deref(), prefix), (Some("172.20.5.9"), Some(20)));
        assert_eq!(parse_resolv("# c\nnameserver 10.255.255.254\nsearch x\nnameserver bad\n"), ["10.255.255.254"]);
        assert_eq!(parse_linux_route(""), (None, None));
    }
}
