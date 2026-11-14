//! 기기 키와 짝 맺기 계산: 지문, 일회용 코드, 코드 증명, 확인 숫자.
//!
//! 인증 기관은 쓰지 않는다. PC마다 스스로 만든 인증서 하나가 있고, 그 공개 키의 지문이 PC의 id다.
//! 개인 키는 Windows에서는 자격 증명 관리자(사용자 계정으로 암호화)에, 그 밖에서는 그 사용자만 여는 파일에 둔다.

use std::path::Path;

use hmac::{Hmac, Mac};
use sha2::{Digest, Sha256};
use tokio_rustls::rustls::pki_types::CertificateDer;
use tokio_rustls::rustls::server::ParsedCertificate;

const CERT_FILE: &str = "device.crt";
#[cfg(not(target_os = "windows"))]
const KEY_FILE: &str = "device.key";
const SECRET_FILE: &str = "alias.key";

/// 코드에 쓰는 글자: 헷갈리는 0·O·1·I·L을 뺀 31자
const CODE_CHARS: &[u8] = b"23456789ABCDEFGHJKMNPQRSTUVWXYZ";
/// 코드 길이. 31자 10개면 약 50비트다.
pub const CODE_LEN: usize = 10;
/// 코드에서 증명용 키를 만들 때 되풀이하는 횟수(받아 적은 통신으로 코드를 맞혀 보는 것을 느리게 한다)
const STRETCH: u32 = 60_000;

#[derive(Clone)]
pub struct Identity {
    pub cert: Vec<u8>,
    /// PKCS#8
    pub key: Vec<u8>,
}

impl Identity {
    pub fn id(&self) -> String {
        fingerprint(&self.cert).unwrap_or_default()
    }
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

fn unhex(s: &str) -> Option<Vec<u8>> {
    if !s.len().is_multiple_of(2) || !s.is_ascii() {
        return None;
    }
    (0..s.len()).step_by(2).map(|i| u8::from_str_radix(s.get(i..i + 2)?, 16).ok()).collect()
}

/// 인증서에 든 공개 키(SubjectPublicKeyInfo)의 SHA-256. 인증서가 아니면 없음.
pub fn fingerprint(cert_der: &[u8]) -> Option<String> {
    let cert = CertificateDer::from(cert_der.to_vec());
    let parsed = ParsedCertificate::try_from(&cert).ok()?;
    Some(hex(&Sha256::digest(parsed.subject_public_key_info().as_ref())))
}

/// 지문 모양(소문자 16진수 64자)인지
pub fn is_fingerprint(s: &str) -> bool {
    s.len() == 64 && s.bytes().all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

pub fn generate() -> Result<Identity, String> {
    let made = rcgen::generate_simple_self_signed(vec!["netx-collect".to_string()]).map_err(|e| format!("기기 키를 만들지 못했습니다: {e}"))?;
    Ok(Identity { cert: made.cert.der().to_vec(), key: made.signing_key.serialize_der() })
}

fn random(buf: &mut [u8]) -> Result<(), String> {
    use tokio_rustls::rustls::crypto::ring::default_provider;
    default_provider().secure_random.fill(buf).map_err(|_| "난수를 얻지 못했습니다".to_string())
}

/// 그 사용자만 열 수 있는 파일로 쓴다. (Windows의 사용자 자료 폴더는 이미 그 사용자와 관리자만 연다.)
pub fn write_private(path: &Path, bytes: &[u8]) -> Result<(), String> {
    let fail = |e: std::io::Error| format!("{}을(를) 쓰지 못했습니다: {e}", path.display());
    #[cfg(unix)]
    {
        use std::io::Write;
        use std::os::unix::fs::OpenOptionsExt;
        let mut f = std::fs::OpenOptions::new().write(true).create(true).truncate(true).mode(0o600).open(path).map_err(fail)?;
        f.write_all(bytes).map_err(fail)
    }
    #[cfg(not(unix))]
    {
        std::fs::write(path, bytes).map_err(fail)
    }
}

#[cfg(target_os = "windows")]
fn key_entry() -> Result<keyring::Entry, String> {
    keyring::Entry::new("NetX", "collect-device-key").map_err(|e| format!("자격 증명 저장소를 열지 못했습니다: {e}"))
}

#[cfg(target_os = "windows")]
fn load_key(_dir: &Path) -> Option<Vec<u8>> {
    unhex(&key_entry().ok()?.get_password().ok()?)
}

#[cfg(target_os = "windows")]
fn save_key(_dir: &Path, key: &[u8]) -> Result<(), String> {
    key_entry()?.set_password(&hex(key)).map_err(|e| format!("기기 키를 저장하지 못했습니다: {e}"))
}

#[cfg(not(target_os = "windows"))]
fn load_key(dir: &Path) -> Option<Vec<u8>> {
    unhex(std::fs::read_to_string(dir.join(KEY_FILE)).ok()?.trim())
}

#[cfg(not(target_os = "windows"))]
fn save_key(dir: &Path, key: &[u8]) -> Result<(), String> {
    write_private(&dir.join(KEY_FILE), hex(key).as_bytes())
}

/// 이 PC의 기기 키. 없거나 짝이 맞지 않으면 새로 만든다(새로 만들면 이전의 짝은 모두 무효다).
pub fn load_or_create(dir: &Path) -> Result<Identity, String> {
    std::fs::create_dir_all(dir).map_err(|e| format!("모음함 폴더를 만들지 못했습니다: {e}"))?;
    if let (Ok(cert), Some(key)) = (std::fs::read(dir.join(CERT_FILE)), load_key(dir)) {
        if fingerprint(&cert).is_some() && !key.is_empty() {
            return Ok(Identity { cert, key });
        }
    }
    let made = generate()?;
    save_key(dir, &made.key)?;
    std::fs::write(dir.join(CERT_FILE), &made.cert).map_err(|e| format!("기기 인증서를 저장하지 못했습니다: {e}"))?;
    Ok(made)
}

/// 가명을 만드는 비밀 값. PC마다 다르고 밖으로 나가지 않는다.
pub fn alias_secret(dir: &Path) -> Result<Vec<u8>, String> {
    let path = dir.join(SECRET_FILE);
    if let Some(secret) = std::fs::read_to_string(&path).ok().and_then(|s| unhex(s.trim())).filter(|s| s.len() == 32) {
        return Ok(secret);
    }
    let mut secret = [0u8; 32];
    random(&mut secret)?;
    std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    write_private(&path, hex(&secret).as_bytes())?;
    Ok(secret.to_vec())
}

/// 일회용 코드를 새로 만든다.
pub fn new_code() -> Result<String, String> {
    let mut out = String::with_capacity(CODE_LEN);
    let mut buf = [0u8; 1];
    while out.len() < CODE_LEN {
        random(&mut buf)?;
        // 31의 배수 밖(248~255)은 버려 고르게 뽑는다.
        if buf[0] < 248 {
            out.push(CODE_CHARS[usize::from(buf[0]) % CODE_CHARS.len()] as char);
        }
    }
    Ok(out)
}

/// 사람이 친 코드를 견줄 모양으로: 빈칸·줄표를 빼고 대문자로.
pub fn normal_code(typed: &str) -> String {
    typed.chars().filter(|c| !c.is_whitespace() && *c != '-').map(|c| c.to_ascii_uppercase()).collect()
}

fn mac(key: &[u8], parts: &[&[u8]]) -> [u8; 32] {
    let mut m = Hmac::<Sha256>::new_from_slice(key).expect("HMAC은 어떤 길이의 키도 받는다");
    for p in parts {
        m.update(p);
        m.update(&[0]);
    }
    m.finalize().into_bytes().into()
}

/// 코드와 두 지문에서 증명용 키를 만든다. 지문이 들어가므로 가운데 낀 자가 받은 증명을 다른 연결에 다시 쓸 수 없다.
pub fn pair_key(code: &str, sender_fp: &str, collector_fp: &str) -> [u8; 32] {
    let salt = mac(b"netx-collect-pair/1", &[sender_fp.as_bytes(), collector_fp.as_bytes()]);
    let mut block = mac(code.as_bytes(), &[&salt, &1u32.to_be_bytes()]);
    let mut out = block;
    for _ in 1..STRETCH {
        block = mac(code.as_bytes(), &[&block]);
        out.iter_mut().zip(block).for_each(|(o, b)| *o ^= b);
    }
    out
}

/// 그 키를 아는 쪽이 내는 증명. `who`는 "sender" 또는 "collector".
pub fn proof(key: &[u8; 32], who: &str) -> String {
    hex(&mac(key, &[who.as_bytes()]))
}

/// 길이가 드러나는 것 말고는 걸린 시간으로 내용이 새지 않게 견준다.
pub fn same(a: &str, b: &str) -> bool {
    a.len() == b.len() && a.bytes().zip(b.bytes()).fold(0u8, |acc, (x, y)| acc | (x ^ y)) == 0
}

/// 두 화면에 띄워 사람이 견주는 확인 숫자(8자리). 두 공개 키 지문에서 계산하므로 가운데 낀 자가 있으면 서로 다르다.
pub fn confirm_number(sender_fp: &str, collector_fp: &str) -> String {
    let d = mac(b"netx-collect-sas/1", &[sender_fp.as_bytes(), collector_fp.as_bytes()]);
    let n = u32::from_be_bytes([d[0], d[1], d[2], d[3]]) % 100_000_000;
    format!("{:04} {:04}", n / 10_000, n % 10_000)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn identity_survives_reload_and_has_a_stable_id() {
        let dir = std::env::temp_dir().join(format!("netx-collect-key-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let a = load_or_create(&dir).unwrap();
        let b = load_or_create(&dir).unwrap();
        assert_eq!(a.id(), b.id());
        assert!(is_fingerprint(&a.id()));
        assert_ne!(generate().unwrap().id(), a.id());
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(std::fs::metadata(dir.join(KEY_FILE)).unwrap().permissions().mode() & 0o777, 0o600);
        }
        assert_eq!(alias_secret(&dir).unwrap(), alias_secret(&dir).unwrap());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn fingerprint_refuses_garbage() {
        assert!(fingerprint(b"").is_none());
        assert!(fingerprint(&[0x30, 0x82, 0xff, 0xff]).is_none());
        assert!(!is_fingerprint("ABC"));
    }

    #[test]
    fn codes_are_long_and_unambiguous() {
        let code = new_code().unwrap();
        assert_eq!(code.len(), CODE_LEN);
        assert!(code.bytes().all(|b| CODE_CHARS.contains(&b)));
        assert_ne!(code, new_code().unwrap());
        assert_eq!(normal_code(" abcd-efgh jk "), "ABCDEFGHJK");
    }

    #[test]
    fn proof_depends_on_code_and_both_keys() {
        let k = pair_key("ABCDEFGHJK", "s", "c");
        assert_eq!(proof(&k, "sender"), proof(&pair_key("ABCDEFGHJK", "s", "c"), "sender"));
        assert_ne!(proof(&k, "sender"), proof(&k, "collector"));
        assert_ne!(proof(&k, "sender"), proof(&pair_key("ABCDEFGHJM", "s", "c"), "sender"));
        assert_ne!(proof(&k, "sender"), proof(&pair_key("ABCDEFGHJK", "s", "x"), "sender"));
        assert!(same("abc", "abc") && !same("abc", "abd") && !same("abc", "ab"));
    }

    #[test]
    fn confirm_number_is_eight_digits_and_order_matters() {
        let n = confirm_number("aa", "bb");
        assert_eq!(n.len(), 9);
        assert!(n.replace(' ', "").bytes().all(|b| b.is_ascii_digit()));
        assert_eq!(n, confirm_number("aa", "bb"));
        assert_ne!(n, confirm_number("aa", "bc"));
    }
}
