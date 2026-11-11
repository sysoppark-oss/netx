//! 자동 업데이트. GitHub의 `<채널>-builds` 브랜치에 올라온 설치 파일이 지금 실행 중인 빌드보다 새것이면
//! 내려받아 SHA-256을 확인한 뒤 설치 프로그램을 실행한다.
//!
//! - 주소는 코드에 고정되어 있고 HTTPS만 쓴다. 다른 곳으로 넘겨 보내는 응답(리다이렉트)은 따르지 않는다.
//! - 채널(main, dev)과 커밋은 빌드할 때 박아 넣는다(build.rs). 직접 빌드한 실행 파일은 채널이 없어 업데이트하지 않는다.
//! - 설치 파일에 별도 전자서명은 없다. 믿는 근거는 GitHub 저장소와 HTTPS, 그리고 같은 곳에 올라온 SHA-256이다.

use std::path::{Path, PathBuf};
use std::time::Duration;

use serde::Serialize;
use sha2::{Digest, Sha256};

pub const COMMIT: &str = env!("NETX_BUILD_COMMIT");
pub const CHANNEL: &str = env!("NETX_BUILD_CHANNEL");
const BUILT_UNIX: &str = env!("NETX_BUILD_UNIX");
const BASE: &str = "https://raw.githubusercontent.com/sysoppark-oss/netx";
/// 설치 파일로 받을 수 있는 가장 큰 크기
const MAX_SETUP_BYTES: usize = 80 * 1024 * 1024;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Manifest {
    pub commit: String,
    pub sha256: String,
    pub built_unix: u64,
}

/// 화면에 알리는 업데이트 상태 (이벤트 `update:status`)
#[derive(Debug, Clone, Serialize)]
pub struct Status {
    /// available, downloading, installing, error
    pub state: &'static str,
    pub commit: String,
    pub message: String,
}

fn is_hex(s: &str, len: usize) -> bool {
    s.len() == len && s.bytes().all(|b| b.is_ascii_hexdigit())
}

/// `키=값` 줄로 된 manifest.txt를 읽는다. 커밋(16진수 40자), SHA-256(64자), 빌드 시각이 모두 맞아야 한다.
pub fn parse_manifest(text: &str) -> Option<Manifest> {
    let get = |key: &str| text.lines().find_map(|l| l.trim().strip_prefix(key)?.strip_prefix('=')).map(str::trim);
    let (commit, sha256) = (get("commit")?.to_ascii_lowercase(), get("setup_sha256")?.to_ascii_lowercase());
    let built_unix = get("built_unix")?.parse().ok()?;
    (is_hex(&commit, 40) && is_hex(&sha256, 64)).then_some(Manifest { commit, sha256, built_unix })
}

/// 올라온 빌드가 내 빌드와 다르고 더 나중에 만들어졌을 때만 새 버전이다(옛 빌드로 되돌아가지 않는다).
pub fn is_newer(m: &Manifest, my_commit: &str, my_built: u64) -> bool {
    !m.commit.eq_ignore_ascii_case(my_commit) && m.built_unix > my_built
}

/// 채널별 설치 파일 이름. 정해진 채널이 아니면 None(업데이트 안 함).
pub fn setup_name(channel: &str) -> Option<&'static str> {
    match channel {
        "main" => Some("NetX-setup.exe"),
        "dev" => Some("NetX-dev-setup.exe"),
        _ => None,
    }
}

pub fn enabled() -> bool {
    cfg!(target_os = "windows") && setup_name(CHANNEL).is_some() && is_hex(COMMIT, 40)
}

fn client() -> Result<reqwest::Client, String> {
    reqwest::Client::builder()
        .https_only(true)
        .redirect(reqwest::redirect::Policy::none())
        .connect_timeout(Duration::from_secs(10))
        .timeout(Duration::from_secs(300))
        .build()
        .map_err(|e| format!("연결을 준비하지 못했습니다: {e}"))
}

async fn fetch(file: &str, limit: usize) -> Result<Vec<u8>, String> {
    let url = format!("{BASE}/{CHANNEL}-builds/{file}");
    let mut res = client()?.get(&url).send().await.map_err(|e| format!("업데이트 서버에 연결하지 못했습니다: {e}"))?;
    if !res.status().is_success() {
        return Err(format!("업데이트 서버가 {} 로 답했습니다", res.status().as_u16()));
    }
    let mut body = Vec::new();
    while let Some(chunk) = res.chunk().await.map_err(|e| format!("내려받다가 끊겼습니다: {e}"))? {
        if body.len() + chunk.len() > limit {
            return Err("내려받은 파일이 너무 큽니다".into());
        }
        body.extend_from_slice(&chunk);
    }
    Ok(body)
}

/// 새 버전이 있으면 그 정보를 돌려준다.
pub async fn check() -> Result<Option<Manifest>, String> {
    if !enabled() {
        return Ok(None);
    }
    let text = String::from_utf8(fetch("manifest.txt", 4096).await?).map_err(|_| "업데이트 정보 형식이 올바르지 않습니다".to_string())?;
    let m = parse_manifest(&text).ok_or("업데이트 정보 형식이 올바르지 않습니다")?;
    Ok(is_newer(&m, COMMIT, BUILT_UNIX.parse().unwrap_or(u64::MAX)).then_some(m))
}

pub fn sha256_hex(data: &[u8]) -> String {
    Sha256::digest(data).iter().map(|b| format!("{b:02x}")).collect()
}

/// 설치 파일을 내려받아 SHA-256이 맞을 때만 `dir`에 저장한다.
pub async fn download(m: &Manifest, dir: &Path) -> Result<PathBuf, String> {
    let name = setup_name(CHANNEL).ok_or("업데이트 채널이 없습니다")?;
    let data = fetch(name, MAX_SETUP_BYTES).await?;
    if sha256_hex(&data) != m.sha256 {
        return Err("내려받은 설치 파일이 올라온 것과 다릅니다(SHA-256 불일치). 설치하지 않았습니다.".into());
    }
    std::fs::create_dir_all(dir).map_err(|e| format!("업데이트 폴더를 만들지 못했습니다: {e}"))?;
    let path = dir.join(name);
    std::fs::write(&path, &data).map_err(|e| format!("설치 파일을 저장하지 못했습니다: {e}"))?;
    Ok(path)
}

/// 설치 프로그램을 실행한다. `/P`는 진행 창만 보이는 설치, `/R`은 설치 뒤 NetX 다시 실행.
pub fn launch(path: &Path) -> Result<(), String> {
    std::process::Command::new(path).args(["/P", "/R"]).spawn().map(|_| ()).map_err(|e| format!("설치 프로그램을 실행하지 못했습니다: {e}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    const C1: &str = "026187f49907216b7c2dd52a2487deefe3fcaa5b";
    const C2: &str = "e60ae11bbd5ba760a09dbfc04a85aeb70c66b0ce";
    const SHA: &str = "22cb6f008b4b17585ceb6bc95da741942625a1b8658b23a642e7739bc0d2017b";

    #[test]
    fn manifest_is_read_strictly() {
        let text = format!("commit={C2}\r\nsetup_sha256={}\r\nbuilt=2026-11-11T02:43:26Z\r\nbuilt_unix=1794365006\r\n", SHA.to_uppercase());
        assert_eq!(parse_manifest(&text), Some(Manifest { commit: C2.into(), sha256: SHA.into(), built_unix: 1794365006 }));
        // 옛 형식(built_unix 없음), 짧은 해시, 엉뚱한 글자는 받지 않는다.
        assert_eq!(parse_manifest(&format!("commit={C2}\nsetup_sha256={SHA}\nbuilt=2026-11-11T02:43:26Z\n")), None);
        assert_eq!(parse_manifest(&format!("commit=abc\nsetup_sha256={SHA}\nbuilt_unix=1\n")), None);
        assert_eq!(parse_manifest(&format!("commit={C2}\nsetup_sha256={}\nbuilt_unix=1\n", "z".repeat(64))), None);
        assert_eq!(parse_manifest("<html>not found</html>"), None);
    }

    #[test]
    fn only_a_different_and_later_build_is_newer() {
        let m = Manifest { commit: C2.into(), sha256: SHA.into(), built_unix: 200 };
        assert!(is_newer(&m, C1, 100));
        assert!(!is_newer(&m, C2, 100), "같은 커밋");
        assert!(!is_newer(&m, &C2.to_uppercase(), 100));
        assert!(!is_newer(&m, C1, 200), "더 나중이 아님");
        assert!(!is_newer(&m, C1, 300), "옛 빌드로 되돌리지 않는다");
    }

    #[test]
    fn channels_and_hash() {
        assert_eq!(setup_name("dev"), Some("NetX-dev-setup.exe"));
        assert_eq!(setup_name("main"), Some("NetX-setup.exe"));
        assert_eq!(setup_name(""), None);
        assert_eq!(setup_name("../x"), None);
        assert_eq!(sha256_hex(b"abc"), "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad");
        // 직접 빌드한 실행 파일(채널 없음)은 업데이트하지 않는다.
        if CHANNEL.is_empty() {
            assert!(!enabled());
        }
    }
}
