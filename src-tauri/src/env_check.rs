//! 실행 환경 확인.
//!
//! WSL2가 있으면 깊은 진단(tcpdump, mtr 등)을 쓸 수 있고,
//! 없으면 Rust 네이티브 기본 진단 모드로 동작한다(DESIGN.md 4.4, DEV_PLAN 3장).
//!
//! 보안 원칙: 셸을 거치지 않고, 명령과 인자를 배열로 분리해 호출한다.

use serde::Serialize;
use std::process::Stdio;
use std::time::Duration;
use tokio::process::Command;
use tokio::time::timeout;

#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "snake_case")]
pub enum Mode {
    /// WSL2 사용 가능: 전체 진단
    Full,
    /// WSL2 없음: Rust 네이티브 기본 진단만
    Basic,
}

#[derive(Serialize, Clone, Debug)]
pub struct EnvironmentStatus {
    pub mode: Mode,
    pub wsl_installed: bool,
    pub distros: Vec<String>,
    pub message: String,
}

const CHECK_TIMEOUT: Duration = Duration::from_secs(8);

/// 마지막으로 확인한 결과: 0 아직 모름, 1 전체 진단, 2 기본 진단
static LAST: std::sync::atomic::AtomicU8 = std::sync::atomic::AtomicU8::new(0);

/// WSL을 쓸 수 있는지(마지막 확인 결과). 아직 확인하지 않았으면 쓸 수 있다고 본다.
/// 계획을 화면에 보일 때 쓴다. 실행 직전에는 `check`로 다시 확인한다.
pub fn full() -> bool {
    LAST.load(std::sync::atomic::Ordering::Relaxed) != 2
}

pub async fn check() -> EnvironmentStatus {
    let status = detect().await;
    LAST.store(if matches!(status.mode, Mode::Full) { 1 } else { 2 }, std::sync::atomic::Ordering::Relaxed);
    status
}

async fn detect() -> EnvironmentStatus {
    // 개발·시험용: 리눅스에서 기본 진단 화면을 보려면 NETX_FORCE_BASIC=1로 켠다.
    #[cfg(not(target_os = "windows"))]
    if std::env::var_os("NETX_FORCE_BASIC").is_some() {
        return EnvironmentStatus { mode: Mode::Basic, wsl_installed: false, distros: vec![], message: "기본 진단 모드를 강제로 켰습니다(개발용).".into() };
    }
    if !cfg!(target_os = "windows") {
        // 리눅스에서는 도구를 직접 실행하므로 전체 진단이 가능하다(개발·테스트용).
        return EnvironmentStatus {
            mode: Mode::Full,
            wsl_installed: false,
            distros: vec![],
            message: "Windows가 아닌 환경입니다. 도구를 직접 실행합니다(개발용).".into(),
        };
    }

    // WSL이 없는 PC에서 wsl.exe를 부르면 "아무 키나 누르면 설치" 화면이 뜬다.
    // 설치를 실수로 시작하지 않도록, 레지스트리에 WSL 배포판 등록 정보가 있을 때만 wsl.exe를 부른다.
    if !wsl_registered().await {
        return EnvironmentStatus {
            mode: Mode::Basic,
            wsl_installed: false,
            distros: vec![],
            message: "WSL2가 없어 기본 진단으로 동작합니다. 핑, 경로 추적, DNS, 포트 확인, 웹 응답, 스캔, 구성도를 쓸 수 있습니다. 패킷 캡처와 대역폭 측정은 전체 진단(WSL2)이 필요합니다.".into(),
        };
    }

    match list_wsl_distros().await {
        Some(distros) if !distros.is_empty() => EnvironmentStatus {
            mode: Mode::Full,
            wsl_installed: true,
            message: format!("WSL2 배포판 {}개를 찾았습니다.", distros.len()),
            distros,
        },
        Some(_) => EnvironmentStatus {
            mode: Mode::Basic,
            wsl_installed: true,
            distros: vec![],
            message: "WSL은 있지만 설치된 리눅스 배포판이 없어 기본 진단으로 동작합니다. 관리자 PowerShell에서 `wsl --install -d Ubuntu`로 설치할 수 있습니다.".into(),
        },
        None => EnvironmentStatus {
            mode: Mode::Basic,
            wsl_installed: false,
            distros: vec![],
            message: "WSL2를 찾지 못해 기본 진단으로 동작합니다.".into(),
        },
    }
}

/// HKCU\Software\Microsoft\Windows\CurrentVersion\Lxss 키가 있으면 배포판이 등록된 적이 있다.
/// reg.exe는 Windows 기본 명령이며, 키가 없으면 실패 코드를 돌려준다.
async fn wsl_registered() -> bool {
    let mut cmd = Command::new("reg.exe");
    cmd.args([
        "query",
        r"HKCU\Software\Microsoft\Windows\CurrentVersion\Lxss",
    ])
    .stdin(Stdio::null())
    .stdout(Stdio::null())
    .stderr(Stdio::null())
    .kill_on_drop(true);
    hide_console_window(&mut cmd);

    matches!(timeout(CHECK_TIMEOUT, cmd.status()).await, Ok(Ok(st)) if st.success())
}

/// `wsl.exe --list --quiet` 결과에서 배포판 이름을 읽는다.
/// wsl.exe는 UTF-16LE로 출력하므로 따로 해석한다.
async fn list_wsl_distros() -> Option<Vec<String>> {
    let mut cmd = Command::new("wsl.exe");
    cmd.args(["--list", "--quiet"])
        .stdin(Stdio::null())
        .kill_on_drop(true);
    hide_console_window(&mut cmd);

    let output = timeout(CHECK_TIMEOUT, cmd.output()).await.ok()?.ok()?;
    if !output.status.success() {
        // 배포판이 하나도 없을 때도 실패 코드가 나올 수 있다.
        return Some(vec![]);
    }
    Some(parse_distro_list(&output.stdout))
}

pub(crate) fn parse_distro_list(raw: &[u8]) -> Vec<String> {
    let text = decode_wsl_output(raw);
    text.lines()
        .map(|l| l.trim().trim_matches('\u{feff}').to_string())
        .filter(|l| !l.is_empty())
        .collect()
}

/// wsl.exe 자체 메시지는 UTF-16LE로, 리눅스 도구의 출력은 UTF-8로 온다.
/// 텍스트 출력의 UTF-8에는 0 바이트가 없으므로, 0 바이트가 있으면 UTF-16으로 본다.
pub(crate) fn decode_wsl_output(raw: &[u8]) -> String {
    if raw.contains(&0) {
        let units: Vec<u16> = raw.chunks_exact(2).map(|c| u16::from_le_bytes([c[0], c[1]])).collect();
        String::from_utf16_lossy(&units)
    } else {
        String::from_utf8_lossy(raw).into_owned()
    }
}

/// Windows에서 외부 명령을 띄울 때 검은 콘솔 창이 깜빡이지 않게 한다.
#[cfg(target_os = "windows")]
pub(crate) fn hide_console_window(cmd: &mut Command) {
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    cmd.creation_flags(CREATE_NO_WINDOW);
}

#[cfg(not(target_os = "windows"))]
pub(crate) fn hide_console_window(_cmd: &mut Command) {}

#[cfg(test)]
mod tests {
    use super::*;

    fn utf16le(s: &str) -> Vec<u8> {
        s.encode_utf16().flat_map(|u| u.to_le_bytes()).collect()
    }

    #[test]
    fn parses_utf16_distro_list() {
        let raw = utf16le("Ubuntu\r\ndocker-desktop\r\n\r\n");
        assert_eq!(parse_distro_list(&raw), vec!["Ubuntu", "docker-desktop"]);
    }

    #[test]
    fn parses_utf8_distro_list() {
        assert_eq!(parse_distro_list(b"Ubuntu-24.04\n"), vec!["Ubuntu-24.04"]);
    }

    #[test]
    fn empty_output_gives_no_distros() {
        assert!(parse_distro_list(b"").is_empty());
    }
}
