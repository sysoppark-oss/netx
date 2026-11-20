//! 전체 진단 준비: 사용자가 고르면 WSL과 Ubuntu를 설치하도록 Windows의 설치 명령을 띄운다.
//!
//! NetX가 관리자 권한을 얻지 않는다. Windows의 `wsl.exe --install`을 "관리자로 실행"으로 띄우고,
//! 승인 창(UAC)의 [예]는 사용자가 누른다. 명령과 인자는 코드에 고정이며 사용자 입력이 들어가지 않는다.
//! 순서: 설치 시작 → (Windows 기능이 켜지면) PC 다시 시작 → NetX가 한 번 스스로 떠서 이어 감 → Ubuntu 등록 →
//! 진단 도구 설치(기존 [도구 설치]). 리눅스 계정을 만들지 않는다(NetX는 root로 도구를 돌린다).

use serde::Serialize;

use crate::env_check::{self, Mode};

/// `wsl --install` 한 줄 설치를 쓸 수 있는 가장 낮은 Windows 빌드(Windows 10 2004판)
pub const MIN_BUILD: u32 = 19041;

#[derive(Serialize, Clone, Debug, PartialEq)]
pub struct WslSetup {
    /// ready(전체 진단 가능), none(시작 전), restart(설치를 띄웠다 — 끝나면 다시 시작), resume(다시 시작한 뒤 — 이어서 설치), unsupported
    pub stage: &'static str,
    /// 사용자가 이 설치를 시작해 아직 끝내지 않았다(끝까지 자동으로 이어 간다)
    pub active: bool,
    pub build: u32,
    pub message: String,
}

/// 지금 어느 단계인가. `started`는 설치를 띄운 때(초, 없으면 0), `boot`는 PC가 켜진 때(초).
pub fn stage(full: bool, build: u32, started: u64, boot: u64) -> &'static str {
    if full {
        "ready"
    } else if build < MIN_BUILD {
        "unsupported"
    } else if started == 0 {
        "none"
    } else if boot > started {
        "resume"
    } else {
        "restart"
    }
}

fn message(stage: &str, build: u32) -> String {
    match stage {
        "ready" => "전체 진단을 쓸 수 있습니다.".into(),
        "unsupported" if build == 0 => "이 PC에서는 자동 설치를 쓸 수 없습니다.".into(),
        "unsupported" => format!("이 Windows(빌드 {build})는 한 번에 설치하는 명령을 지원하지 않습니다. Windows를 업데이트한 뒤 다시 해 보세요(빌드 {MIN_BUILD} 이상)."),
        "restart" => "설치 창(검은 창)이 끝나고 닫히면 PC를 다시 시작해야 합니다.".into(),
        "resume" => "PC가 다시 시작됐습니다. 리눅스(Ubuntu)를 마저 설치합니다.".into(),
        _ => "아직 시작하지 않았습니다.".into(),
    }
}

pub async fn status() -> WslSetup {
    let full = matches!(env_check::check().await.mode, Mode::Full);
    if cfg!(debug_assertions)
        && let Ok(forced) = std::env::var("NETX_TEST_WSLSETUP")
        && let Some(s) = ["none", "restart", "resume", "unsupported"].into_iter().find(|s| *s == forced)
    {
        return WslSetup { stage: s, active: s != "none" && s != "unsupported", build: MIN_BUILD, message: message(s, MIN_BUILD) };
    }
    let (build, started) = (os::build(), os::started());
    let s = stage(full, build, started, sysinfo::System::boot_time());
    WslSetup { stage: s, active: started > 0, build, message: message(s, build) }
}

/// 설치를 띄운다(처음이든 다시 시작한 뒤든 같은 명령이다 — 이미 된 단계는 Windows가 건너뛴다).
pub fn start() -> Result<(), String> {
    os::install()
}

/// 다시 켜지면 NetX가 한 번 스스로 뜨게 해 두고, 20초 뒤 PC를 다시 시작한다.
pub fn restart() -> Result<(), String> {
    os::restart()
}

/// 끝났거나 그만둔다: 이어 가기 표시를 지운다.
pub fn clear() {
    os::clear();
}

#[cfg(target_os = "windows")]
mod os {
    use std::path::PathBuf;

    use winreg::RegKey;
    use winreg::enums::{HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE};

    const OWN_KEY: &str = r"SOFTWARE\Antioch\NetX";
    const STARTED: &str = "WslSetupStarted";
    const RUN_ONCE: &str = r"Software\Microsoft\Windows\CurrentVersion\RunOnce";
    const RUN_NAME: &str = "NetXWslSetup";
    /// 고정 인자. `--no-launch`: 설치 뒤 Ubuntu 창(계정 만들기)을 띄우지 않는다.
    const INSTALL_ARGS: &str = "--install -d Ubuntu --no-launch";

    fn now() -> u64 {
        std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0)
    }

    /// Windows 폴더의 System32 안 프로그램. 경로는 관리자만 고칠 수 있는 HKLM 값에서 읽는다(PATH를 따르지 않는다).
    fn system(program: &str) -> Result<PathBuf, String> {
        let root: String = RegKey::predef(HKEY_LOCAL_MACHINE)
            .open_subkey(r"SOFTWARE\Microsoft\Windows NT\CurrentVersion")
            .and_then(|k| k.get_value("SystemRoot"))
            .map_err(|_| "Windows 폴더를 찾지 못했습니다".to_string())?;
        let path = PathBuf::from(root).join("System32").join(program);
        if path.is_file() { Ok(path) } else { Err(format!("{program}를 찾지 못했습니다. 이 Windows에는 WSL 설치 명령이 없습니다.")) }
    }

    pub fn build() -> u32 {
        RegKey::predef(HKEY_LOCAL_MACHINE)
            .open_subkey(r"SOFTWARE\Microsoft\Windows NT\CurrentVersion")
            .and_then(|k| k.get_value::<String, _>("CurrentBuildNumber"))
            .ok()
            .and_then(|v| v.trim().parse().ok())
            .unwrap_or(0)
    }

    pub fn started() -> u64 {
        RegKey::predef(HKEY_CURRENT_USER).open_subkey(OWN_KEY).and_then(|k| k.get_value::<String, _>(STARTED)).ok().and_then(|v| v.parse().ok()).unwrap_or(0)
    }

    pub fn install() -> Result<(), String> {
        use windows::Win32::UI::Shell::ShellExecuteW;
        use windows::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;
        use windows::core::{HSTRING, PCWSTR, w};
        if build() < super::MIN_BUILD {
            return Err(super::message("unsupported", build()));
        }
        let file = HSTRING::from(system("wsl.exe")?.as_os_str());
        let args = HSTRING::from(INSTALL_ARGS);
        // "runas": 관리자로 실행. Windows가 승인 창을 띄우고 사용자가 [예]를 눌러야 시작된다.
        let result = unsafe { ShellExecuteW(None, w!("runas"), &file, &args, PCWSTR::null(), SW_SHOWNORMAL) };
        match result.0 as isize {
            code if code > 32 => {
                let (key, _) = RegKey::predef(HKEY_CURRENT_USER).create_subkey(OWN_KEY).map_err(|e| format!("설치 상태를 적지 못했습니다: {e}"))?;
                // 다시 시작한 뒤 이어서 띄울 때는 처음 시작한 때를 그대로 둔다(그래야 "다시 시작했음"을 안다).
                if started() == 0 {
                    key.set_value(STARTED, &now().to_string()).map_err(|e| format!("설치 상태를 적지 못했습니다: {e}"))?;
                }
                Ok(())
            }
            5 => Err("설치를 시작하지 못했습니다. 관리자 승인 창에서 [예]를 눌러야 합니다.".into()),
            code => Err(format!("설치 명령을 실행하지 못했습니다 (오류 {code}).")),
        }
    }

    pub fn restart() -> Result<(), String> {
        if started() == 0 {
            return Err("먼저 설치를 시작해야 합니다".into());
        }
        let exe = std::env::current_exe().map_err(|_| "NetX의 위치를 찾지 못했습니다".to_string())?;
        if let Ok((key, _)) = RegKey::predef(HKEY_CURRENT_USER).create_subkey(RUN_ONCE) {
            let _ = key.set_value(RUN_NAME, &format!("\"{}\"", exe.display()));
        }
        let mut cmd = std::process::Command::new(system("shutdown.exe")?);
        cmd.args(["/r", "/t", "20", "/c", "NetX: 전체 진단(WSL) 설치를 마치려고 20초 뒤 다시 시작합니다."]);
        std::os::windows::process::CommandExt::creation_flags(&mut cmd, 0x0800_0000);
        cmd.spawn().map(|_| ()).map_err(|e| format!("다시 시작하지 못했습니다: {e}"))
    }

    pub fn clear() {
        if let Ok(key) = RegKey::predef(HKEY_CURRENT_USER).open_subkey_with_flags(OWN_KEY, winreg::enums::KEY_SET_VALUE) {
            let _ = key.delete_value(STARTED);
        }
        if let Ok(key) = RegKey::predef(HKEY_CURRENT_USER).open_subkey_with_flags(RUN_ONCE, winreg::enums::KEY_SET_VALUE) {
            let _ = key.delete_value(RUN_NAME);
        }
    }
}

#[cfg(not(target_os = "windows"))]
mod os {
    pub fn build() -> u32 {
        0
    }
    pub fn started() -> u64 {
        0
    }
    pub fn install() -> Result<(), String> {
        Err("WSL 설치는 Windows에서만 합니다".into())
    }
    pub fn restart() -> Result<(), String> {
        Err("WSL 설치는 Windows에서만 합니다".into())
    }
    pub fn clear() {}
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stages_follow_the_install_order() {
        assert_eq!(stage(true, 0, 0, 0), "ready");
        assert_eq!(stage(false, 18363, 0, 100), "unsupported");
        assert_eq!(stage(false, MIN_BUILD, 0, 100), "none");
        // 설치를 띄운 뒤 아직 다시 시작하지 않았다
        assert_eq!(stage(false, MIN_BUILD, 200, 100), "restart");
        // 다시 시작했다(켜진 때가 설치를 띄운 때보다 뒤)
        assert_eq!(stage(false, 26100, 200, 300), "resume");
        assert_eq!(stage(true, 26100, 200, 300), "ready");
        assert!(message("unsupported", 18363).contains("19041"));
    }
}
