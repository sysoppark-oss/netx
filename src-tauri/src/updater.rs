//! 자동 업데이트. GitHub의 `<채널>-builds` 브랜치에 올라온 설치 파일이 지금 실행 중인 빌드보다 새것이면
//! 내려받아 SHA-256을 확인한 뒤 설치 프로그램을 실행한다.
//!
//! - 주소는 코드에 고정되어 있고 HTTPS만 쓴다. 다른 곳으로 넘겨 보내는 응답(리다이렉트)은 따르지 않는다.
//! - 채널(main, dev)과 커밋은 빌드할 때 박아 넣는다(build.rs). 직접 빌드한 실행 파일은 채널이 없어 업데이트하지 않는다.
//! - 설치 파일은 업데이트 서명(Tauri 서명 도구가 만드는 minisign 서명, 스토리방화벽과 같은 방식)을 확인한 뒤에만 실행한다.
//!   공개 키는 이 파일에 들어 있고(`PUBLIC_KEY`), 개인 키는 저장소 비밀값에만 있다. 서명 파일(`<설치 파일>.sig`)이
//!   없거나 맞지 않으면 설치하지 않는다. 같은 곳에 올라온 SHA-256도 함께 확인한다.
//! - 실행 중인 작업이 있으면 설치를 미룬다(`decide`). 설치하면 NetX가 닫히기 때문이다.
//! - 보내는 것은 정해진 주소의 파일을 달라는 요청뿐이다. 이 PC의 이름, 설정, 진단 결과는 보내지 않는다.

use std::path::{Path, PathBuf};
use std::time::Duration;

use base64::Engine;
use serde::Serialize;
use sha2::{Digest, Sha256};

pub const COMMIT: &str = env!("NETX_BUILD_COMMIT");
pub const CHANNEL: &str = env!("NETX_BUILD_CHANNEL");
const BUILT_UNIX: &str = env!("NETX_BUILD_UNIX");
const BASE: &str = "https://raw.githubusercontent.com/sysoppark-oss/netx";
/// 업데이트 서명을 확인하는 공개 키(Tauri 형식: minisign 공개 키 파일을 base64로 감싼 것). 공개해도 되는 값이다.
/// 짝이 되는 개인 키는 GitHub 저장소 비밀값 `TAURI_SIGNING_PRIVATE_KEY`에만 있다. 키를 바꾸면 이미 설치된 판은
/// 새 서명을 확인하지 못하므로 손으로 다시 설치해야 한다.
pub const PUBLIC_KEY: &str = "dW50cnVzdGVkIGNvbW1lbnQ6IG1pbmlzaWduIHB1YmxpYyBrZXk6IDIwMDlENDUzNThGQjEyRDEKUldUUkV2dFlVOVFKSUdMZFhDeUdQbFJZazhQeksvU1ZRWmRnZHBORUpVZFcyMFRPVDFaSGhCbnEK";
/// 서명 파일로 받을 수 있는 가장 큰 크기
const MAX_SIG_BYTES: usize = 4096;
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
    /// notice(알림만), available(설치 버튼), downloading, installing, error
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

/// 이 채널에서 NetX가 스스로 설치까지 해도 되는지.
/// 서명 검증이 들어간 뒤로 개발판(dev)과 정식판(main) 모두 허용한다. 그 밖의 채널은 알리기만 한다.
pub fn installs(channel: &str) -> bool {
    matches!(channel, "dev" | "main")
}

/// 새 버전을 찾았을 때 할 일
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Action {
    /// 알리기만 한다. 설치 버튼도 없다(정해진 채널이 아닐 때).
    Notice,
    /// 바로 내려받아 설치한다.
    Install,
    /// 실행 중인 작업이 있어 미룬다. [지금 업데이트] 버튼으로 알린다.
    Defer,
    /// [지금 업데이트] 버튼으로 알린다.
    Offer,
}

/// `at_start`: 시작 직후의 확인인지, `manual`: 설정에서 자동 설치를 껐는지, `busy`: 실행 중인 작업이 있는지
pub fn decide(channel: &str, at_start: bool, manual: bool, busy: bool) -> Action {
    if !installs(channel) {
        Action::Notice
    } else if !at_start || manual {
        Action::Offer
    } else if busy {
        Action::Defer
    } else {
        Action::Install
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

fn unwrap_base64(text: &str) -> Option<String> {
    String::from_utf8(base64::engine::general_purpose::STANDARD.decode(text.trim()).ok()?).ok()
}

/// `data`가 `public_key`의 짝인 개인 키로 서명된 것인지 확인한다.
/// `sig_file`은 Tauri 서명 도구가 만든 `.sig` 파일의 내용, `public_key`는 Tauri 형식 공개 키다.
pub fn verify_signature(data: &[u8], sig_file: &str, public_key: &str) -> Result<(), String> {
    const BAD: &str = "설치 파일의 서명을 확인하지 못했습니다. 설치하지 않았습니다.";
    let key = unwrap_base64(public_key).and_then(|k| minisign_verify::PublicKey::decode(&k).ok()).ok_or("업데이트 공개 키가 올바르지 않습니다.")?;
    let sig = unwrap_base64(sig_file).and_then(|s| minisign_verify::Signature::decode(&s).ok()).ok_or(BAD)?;
    key.verify(data, &sig, true).map_err(|_| BAD.to_string())
}

/// 설치 파일과 서명을 내려받아 SHA-256과 서명이 모두 맞을 때만 `dir`에 저장한다.
pub async fn download(m: &Manifest, dir: &Path) -> Result<PathBuf, String> {
    let name = setup_name(CHANNEL).ok_or("업데이트 채널이 없습니다")?;
    let data = fetch(name, MAX_SETUP_BYTES).await?;
    if sha256_hex(&data) != m.sha256 {
        return Err("내려받은 설치 파일이 올라온 것과 다릅니다(SHA-256 불일치). 설치하지 않았습니다.".into());
    }
    let sig = fetch(&format!("{name}.sig"), MAX_SIG_BYTES).await.map_err(|e| format!("설치 파일의 서명을 받지 못했습니다. 설치하지 않았습니다. ({e})"))?;
    verify_signature(&data, &String::from_utf8_lossy(&sig), PUBLIC_KEY)?;
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
    fn both_channels_install_and_running_work_defers() {
        assert!(installs("main") && installs("dev") && !installs("") && !installs("DEV") && !installs("feature"));
        // 정해진 채널이 아니면 어떤 경우에도 설치하지 않는다.
        for (start, manual, busy) in [(true, false, false), (true, true, false), (false, false, true), (true, false, true)] {
            assert_eq!(decide("feature", start, manual, busy), Action::Notice);
        }
        // 시작 직후이고 자동 설치가 켜져 있고 한가할 때만 바로 설치한다.
        for channel in ["dev", "main"] {
            assert_eq!(decide(channel, true, false, false), Action::Install);
            assert_eq!(decide(channel, true, false, true), Action::Defer);
            assert_eq!(decide(channel, true, true, false), Action::Offer);
            assert_eq!(decide(channel, true, true, true), Action::Offer);
            assert_eq!(decide(channel, false, false, false), Action::Offer);
            assert_eq!(decide(channel, false, false, true), Action::Offer);
        }
    }

    // 시험용으로 한 번 만들고 버린 키와, 그 키로 `tauri signer sign`이 만든 서명
    const TEST_PUB: &str = "dW50cnVzdGVkIGNvbW1lbnQ6IG1pbmlzaWduIHB1YmxpYyBrZXk6IDU3RDQyNDM4OEFBRjhBMjAKUldRZ2lxK0tPQ1RVVjBpY2w3L3FkME1xcEtndTIvV2JGcVByd2hEd2Z1cTVpN0N4VGpOQndsazUK";
    const TEST_SIG: &str = "dW50cnVzdGVkIGNvbW1lbnQ6IHNpZ25hdHVyZSBmcm9tIHRhdXJpIHNlY3JldCBrZXkKUlVRZ2lxK0tPQ1RVVndrMGUxQU5lbHdBaXUxdTZLQXJlNXk0MDIvR3hMY2lWd1NVazBTd2dCUTkvd2hweEFFeUladWF1bmlhcjgzUUNkdEdRVDhwdXFZZUtiakdMeHlzNVFFPQp0cnVzdGVkIGNvbW1lbnQ6IHRpbWVzdGFtcDoxNzk0Mzc1ODkzCWZpbGU6cGF5bG9hZC5iaW4Kb0dwRnhtTFBjUlBRTHVmSmQrVFRZemw1blhISGNjY0lyYmNZZ2dVanl1OGZaMzJ4WCtyZ0x3R1A1MUlpSjdYc0lUR3BOVzFUU3RnVnlxR2VzMkN4QkE9PQo=";
    const TEST_DATA: &[u8] = b"netx signature test payload\n";

    #[test]
    fn signature_must_match_data_and_key() {
        assert_eq!(verify_signature(TEST_DATA, TEST_SIG, TEST_PUB), Ok(()));
        assert_eq!(verify_signature(TEST_DATA, &format!("  {TEST_SIG}\r\n"), TEST_PUB), Ok(()), "앞뒤 공백과 줄바꿈은 괜찮다");
        // 내용이 한 글자라도 다르면 거절
        assert!(verify_signature(b"netx signature test payloae\n", TEST_SIG, TEST_PUB).is_err());
        assert!(verify_signature(b"", TEST_SIG, TEST_PUB).is_err());
        // 다른 키(실제 배포 키)로는 확인되지 않는다.
        assert!(verify_signature(TEST_DATA, TEST_SIG, PUBLIC_KEY).is_err());
        // 서명이 없거나 깨졌으면 거절
        for bad in ["", "not base64 !!", "aGVsbG8=", "<html>404</html>", &TEST_SIG[..TEST_SIG.len() - 8]] {
            assert!(verify_signature(TEST_DATA, bad, TEST_PUB).is_err(), "{bad}");
        }
        // 서명 한 글자를 바꾼 것
        let mut forged = TEST_SIG.to_string();
        forged.replace_range(120..121, if &TEST_SIG[120..121] == "A" { "B" } else { "A" });
        assert!(verify_signature(TEST_DATA, &forged, TEST_PUB).is_err());
        // 공개 키가 깨졌으면 그 사실을 알린다.
        assert!(verify_signature(TEST_DATA, TEST_SIG, "xx").unwrap_err().contains("공개 키"));
    }

    #[test]
    fn embedded_public_key_is_readable() {
        let text = unwrap_base64(PUBLIC_KEY).expect("base64");
        assert!(text.starts_with("untrusted comment: minisign public key: 2009D45358FB12D1"));
        assert!(minisign_verify::PublicKey::decode(&text).is_ok());
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
