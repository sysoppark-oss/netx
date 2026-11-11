//! 음성 입력: Windows 음성 입력(Win+H)을 연다.
//!
//! NetX는 마이크를 열지 않고 소리를 받거나 저장하지 않는다. 고정된 단축키(Win+H)를 한 번 눌러
//! Windows의 음성 입력 막대를 띄울 뿐이고, 말한 것은 Windows가 지금 선택된 입력칸에 글자로 넣는다.
//! Windows 음성 입력은 소리를 Microsoft 음성 서비스로 보낸다. 그래서 설정에서 켠 경우에만 동작한다(기본 꺼짐).

use tauri::AppHandle;

/// 화면이 입력칸에 초점을 둔 뒤 부른다.
#[tauri::command]
pub fn voice_typing(app: AppHandle) -> Result<(), String> {
    use tauri::Manager;
    let dir = app.path().app_local_data_dir().map_err(|e| format!("앱 데이터 폴더를 찾지 못했습니다: {e}"))?;
    if !crate::settings::load(&dir).voice_input {
        return Err("음성 입력이 꺼져 있습니다. 설정에서 켜세요.".into());
    }
    press()
}

#[cfg(target_os = "windows")]
fn press() -> Result<(), String> {
    use windows::Win32::UI::Input::KeyboardAndMouse::{INPUT, INPUT_0, INPUT_KEYBOARD, KEYBD_EVENT_FLAGS, KEYBDINPUT, KEYEVENTF_KEYUP, SendInput, VIRTUAL_KEY, VK_LWIN};

    let key = |vk: VIRTUAL_KEY, up: bool| INPUT {
        r#type: INPUT_KEYBOARD,
        Anonymous: INPUT_0 { ki: KEYBDINPUT { wVk: vk, wScan: 0, dwFlags: if up { KEYEVENTF_KEYUP } else { KEYBD_EVENT_FLAGS(0) }, time: 0, dwExtraInfo: 0 } },
    };
    // 고정된 키: Win 누름, H 누름, H 뗌, Win 뗌
    let h = VIRTUAL_KEY(0x48);
    let inputs = [key(VK_LWIN, false), key(h, false), key(h, true), key(VK_LWIN, true)];
    let sent = unsafe { SendInput(&inputs, std::mem::size_of::<INPUT>() as i32) };
    if sent as usize == inputs.len() {
        Ok(())
    } else {
        Err("Windows 음성 입력을 열지 못했습니다. 입력칸을 누른 뒤 키보드의 Windows 키와 H를 함께 눌러 보세요.".into())
    }
}

#[cfg(not(target_os = "windows"))]
fn press() -> Result<(), String> {
    Err("음성 입력은 Windows에서만 됩니다.".into())
}
