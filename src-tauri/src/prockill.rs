//! Windows 프로세스 종료. 되돌릴 수 없는 조치라서 Windows Hello(지문, 얼굴, PIN)로 본인 확인을 받는다.
//!
//! - Windows Hello를 쓸 수 없는 PC에서는 프로세스 이름을 직접 입력해야 종료한다.
//! - 시스템 프로세스와 NetX 자신은 종료하지 않는다.
//! - 실행하는 명령은 `tasklist`, `taskkill`뿐이고 인자는 숫자로 바꾼 PID만 들어간다.

use serde::Serialize;

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum KillResult {
    /// 종료함
    Killed { message: String },
    /// Windows Hello를 쓸 수 없어 프로세스 이름 입력이 필요함
    NeedName { name: String, message: String },
    /// 본인 확인을 통과하지 못했거나 취소함
    Denied { message: String },
}

/// 종료하면 Windows가 멈추거나 로그아웃되는 프로세스
const PROTECTED: &[&str] = &[
    "system", "registry", "secure system", "memory compression", "smss.exe", "csrss.exe", "wininit.exe", "winlogon.exe", "services.exe",
    "lsass.exe", "lsaiso.exe", "svchost.exe", "dwm.exe", "fontdrvhost.exe", "msmpeng.exe", "system idle process",
];

pub fn is_protected(pid: u32, name: &str) -> bool {
    pid <= 4 || pid == std::process::id() || PROTECTED.contains(&name.trim().to_lowercase().as_str())
}

/// `tasklist /FI "PID eq N" /FO CSV /NH`의 첫 칸(프로세스 이름)을 읽는다.
#[cfg_attr(not(target_os = "windows"), allow(dead_code))]
pub fn parse_tasklist(stdout: &str, pid: u32) -> Option<String> {
    stdout.lines().find_map(|line| {
        let mut cols = line.trim().split("\",\"");
        let name = cols.next()?.trim_start_matches('"');
        let found: u32 = cols.next()?.trim_matches('"').parse().ok()?;
        (found == pid && !name.is_empty()).then(|| name.to_string())
    })
}

/// 본인 확인 결과
#[derive(Debug, Clone, Copy, PartialEq)]
#[cfg_attr(not(target_os = "windows"), allow(dead_code))]
pub enum Consent {
    Verified,
    /// 이 PC에서 Windows Hello를 쓸 수 없음(장치 없음, 설정 안 함, 정책으로 막힘)
    Unavailable,
    /// 취소했거나 실패함
    Denied,
}

/// 확인을 통과했는지에 따라 종료해도 되는지 정한다.
pub fn decide(consent: Consent, name: &str, typed: Option<&str>) -> Result<(), KillResult> {
    match consent {
        Consent::Verified => Ok(()),
        Consent::Denied => Err(KillResult::Denied { message: "본인 확인을 통과하지 못해 종료하지 않았습니다.".into() }),
        Consent::Unavailable => match typed {
            Some(t) if t.trim().eq_ignore_ascii_case(name) => Ok(()),
            Some(_) => Err(KillResult::NeedName { name: name.into(), message: "입력한 이름이 프로세스 이름과 다릅니다.".into() }),
            None => Err(KillResult::NeedName {
                name: name.into(),
                message: "이 PC에서는 Windows Hello를 쓸 수 없습니다. 종료하려면 프로세스 이름을 그대로 입력하세요.".into(),
            }),
        },
    }
}

#[cfg(target_os = "windows")]
mod imp {
    use super::Consent;
    use std::os::windows::process::CommandExt;
    use std::process::{Command, Stdio};
    use windows::core::{factory, HSTRING};
    use windows::Security::Credentials::UI::{UserConsentVerificationResult, UserConsentVerifier, UserConsentVerifierAvailability};
    use windows::Win32::Foundation::HWND;
    use windows::Win32::System::WinRT::IUserConsentVerifierInterop;

    const CREATE_NO_WINDOW: u32 = 0x0800_0000;

    /// Windows Hello 확인 창을 NetX 창 위에 띄우고 결과를 기다린다. 기다리는 동안 스레드를 붙잡으므로 별도 스레드에서 부른다.
    pub fn consent(hwnd: isize, reason: &str) -> Consent {
        let available = UserConsentVerifier::CheckAvailabilityAsync().and_then(|op| op.join());
        match available {
            Ok(a) if a == UserConsentVerifierAvailability::Available => {}
            Ok(a) if a == UserConsentVerifierAvailability::DeviceBusy => return Consent::Denied,
            _ => return Consent::Unavailable,
        }
        let asked = factory::<UserConsentVerifier, IUserConsentVerifierInterop>().and_then(|interop| unsafe {
            interop.RequestVerificationForWindowAsync::<windows_future::IAsyncOperation<UserConsentVerificationResult>>(
                HWND(hwnd as *mut core::ffi::c_void),
                &HSTRING::from(reason),
            )
        });
        match asked.and_then(|op| op.join()) {
            Ok(r) if r == UserConsentVerificationResult::Verified => Consent::Verified,
            Ok(r) if r == UserConsentVerificationResult::DeviceNotPresent
                || r == UserConsentVerificationResult::NotConfiguredForUser
                || r == UserConsentVerificationResult::DisabledByPolicy =>
            {
                Consent::Unavailable
            }
            _ => Consent::Denied,
        }
    }

    fn run(program: &str, args: &[&str]) -> Result<(bool, String), String> {
        let out = Command::new(program)
            .args(args)
            .stdin(Stdio::null())
            .creation_flags(CREATE_NO_WINDOW)
            .output()
            .map_err(|e| format!("{program}을(를) 실행하지 못했습니다: {e}"))?;
        // tasklist·taskkill의 출력은 한글 Windows에서 CP949라 글자가 깨질 수 있어 성공 여부만 쓴다.
        Ok((out.status.success(), String::from_utf8_lossy(&out.stdout).into_owned()))
    }

    pub fn process_name(pid: u32) -> Result<Option<String>, String> {
        let filter = format!("PID eq {pid}");
        let (_, stdout) = run("tasklist.exe", &["/FI", &filter, "/FO", "CSV", "/NH"])?;
        Ok(super::parse_tasklist(&stdout, pid))
    }

    pub fn kill(pid: u32) -> Result<bool, String> {
        run("taskkill.exe", &["/PID", &pid.to_string(), "/F"]).map(|(ok, _)| ok)
    }
}

#[cfg(not(target_os = "windows"))]
mod imp {
    use super::Consent;
    pub fn consent(_hwnd: isize, _reason: &str) -> Consent {
        Consent::Unavailable
    }
    pub fn process_name(_pid: u32) -> Result<Option<String>, String> {
        Err("프로세스 종료는 Windows에서만 지원합니다".into())
    }
    pub fn kill(_pid: u32) -> Result<bool, String> {
        Err("프로세스 종료는 Windows에서만 지원합니다".into())
    }
}

/// 프로세스를 종료한다. 이 함수는 본인 확인 창이 닫힐 때까지 기다리므로 별도 스레드에서 부른다.
/// 돌려주는 값: (결과, 종료한 프로세스 이름)
pub fn kill_process(hwnd: isize, pid: u32, typed_name: Option<&str>) -> Result<(KillResult, Option<String>), String> {
    let name = imp::process_name(pid)?.ok_or("그 번호의 프로세스가 없습니다. 이미 끝났을 수 있습니다.")?;
    if is_protected(pid, &name) {
        return Err(format!("{name}은(는) 시스템 프로세스라서 종료할 수 없습니다."));
    }
    let consent = imp::consent(hwnd, &format!("NetX가 {name} (PID {pid}) 프로세스를 종료하려고 합니다."));
    if let Err(result) = decide(consent, &name, typed_name) {
        return Ok((result, None));
    }
    // 확인하는 사이에 같은 번호가 다른 프로세스로 바뀌지 않았는지 다시 본다.
    if imp::process_name(pid)?.as_deref() != Some(name.as_str()) {
        return Err("확인하는 사이에 프로세스가 바뀌어 종료하지 않았습니다.".into());
    }
    if imp::kill(pid)? {
        Ok((KillResult::Killed { message: format!("{name} (PID {pid})을(를) 종료했습니다.") }, Some(name)))
    } else {
        Err(format!("{name} (PID {pid})을(를) 종료하지 못했습니다. 관리자 권한으로 실행된 프로그램일 수 있습니다."))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_the_name_for_the_exact_pid() {
        let out = "\"node.exe\",\"8080\",\"Console\",\"1\",\"52,100 K\"\r\n";
        assert_eq!(parse_tasklist(out, 8080).as_deref(), Some("node.exe"));
        assert_eq!(parse_tasklist(out, 80), None);
        assert_eq!(parse_tasklist("정보: 지정된 조건에 맞는 작업이 없습니다.\r\n", 1), None);
        assert_eq!(parse_tasklist("", 1), None);
    }

    #[test]
    fn system_processes_and_netx_itself_are_protected() {
        for (pid, name) in [(4, "System"), (0, "x.exe"), (700, "LSASS.EXE"), (812, "svchost.exe"), (600, "csrss.exe"), (std::process::id(), "netx.exe")] {
            assert!(is_protected(pid, name), "{pid} {name}");
        }
        assert!(!is_protected(8080, "node.exe"));
    }

    #[test]
    fn kill_needs_consent_or_the_typed_name() {
        assert!(decide(Consent::Verified, "node.exe", None).is_ok());
        assert!(matches!(decide(Consent::Denied, "node.exe", Some("node.exe")), Err(KillResult::Denied { .. })));
        assert!(matches!(decide(Consent::Unavailable, "node.exe", None), Err(KillResult::NeedName { .. })));
        assert!(matches!(decide(Consent::Unavailable, "node.exe", Some("nod")), Err(KillResult::NeedName { .. })));
        assert!(decide(Consent::Unavailable, "node.exe", Some(" NODE.EXE ")).is_ok());
    }
}
