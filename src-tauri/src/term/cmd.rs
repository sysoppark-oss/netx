//! 터미널 창과 `term` 모듈을 잇는 명령. `open_terminal`만 본창이 부르고, 나머지는 터미널 창만 부를 수 있다
//! (`capabilities/terminal.json`).

use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use base64::Engine;
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter, Manager};

use super::{Sink, Spec};

const WINDOW: &str = "terminal";

/// 스캔 결과에서 넘어온 접속 정보. 터미널 창이 이 정보로 접속을 시작한다(`term_open`에서 다시 검사한다).
#[derive(Deserialize, Serialize, Clone, Debug)]
pub struct Preset {
    pub kind: String,
    pub host: String,
    pub port: u16,
    pub user: Option<String>,
}

static PRESET: Mutex<Option<Preset>> = Mutex::new(None);

#[derive(Serialize, Clone)]
struct DataEvent {
    id: u32,
    /// base64
    data: String,
}

#[derive(Serialize, Clone)]
struct EndEvent<'a> {
    id: u32,
    message: &'a str,
}

struct WindowSink(AppHandle);

impl Sink for WindowSink {
    fn data(&self, id: u32, bytes: &[u8]) {
        let _ = self.0.emit_to(WINDOW, "term:data", DataEvent { id, data: base64::engine::general_purpose::STANDARD.encode(bytes) });
    }

    fn end(&self, id: u32, message: &str) {
        let _ = self.0.emit_to(WINDOW, "term:end", EndEvent { id, message });
    }
}

fn dirs(app: &AppHandle) -> Result<(PathBuf, PathBuf), String> {
    let data = app.path().app_local_data_dir().map_err(|e| format!("앱 데이터 폴더를 찾지 못했습니다: {e}"))?;
    let transfer = data.join("work").join("transfer");
    std::fs::create_dir_all(&transfer).map_err(|e| format!("전송 폴더를 만들지 못했습니다: {e}"))?;
    Ok((data, transfer))
}

/// 터미널 창을 연다(이미 있으면 앞으로 가져온다). `preset`이 있으면 그 접속을 시작한다.
#[tauri::command]
pub async fn open_terminal(app: AppHandle, preset: Option<Preset>) -> Result<(), String> {
    if let Some(p) = &preset {
        if !matches!(p.kind.as_str(), "ssh" | "telnet" | "ftp" | "tftp") {
            return Err("지원하지 않는 접속 방식입니다".into());
        }
        super::check_host(&p.host)?;
        if p.port == 0 {
            return Err("포트 번호가 올바르지 않습니다".into());
        }
        if let Some(user) = p.user.as_deref().filter(|u| !u.is_empty()) {
            super::check_user(user)?;
        }
    }
    *PRESET.lock().unwrap_or_else(|e| e.into_inner()) = preset;
    if let Some(window) = app.get_webview_window(WINDOW) {
        let _ = window.unminimize();
        let _ = window.set_focus();
        let _ = app.emit_to(WINDOW, "term:preset", ());
        return Ok(());
    }
    let window = tauri::WebviewWindowBuilder::new(&app, WINDOW, tauri::WebviewUrl::App("index.html".into()))
        .title("NetX 터미널")
        .inner_size(1000.0, 680.0)
        .min_inner_size(720.0, 420.0)
        .build()
        .map_err(|e| format!("터미널 창을 열지 못했습니다: {e}"))?;
    // 창을 닫으면 열려 있던 접속을 모두 닫는다.
    window.on_window_event(|event| {
        if matches!(event, tauri::WindowEvent::Destroyed) {
            super::close_all();
        }
    });
    Ok(())
}

/// 본창이 넘긴 접속 정보를 한 번 꺼낸다.
#[tauri::command]
pub fn term_take_preset() -> Option<Preset> {
    PRESET.lock().unwrap_or_else(|e| e.into_inner()).take()
}

/// 이 PC의 시리얼 포트 목록
#[tauri::command]
pub async fn term_ports() -> Vec<String> {
    tauri::async_runtime::spawn_blocking(super::serial::ports).await.unwrap_or_default()
}

/// 접속을 연다. 접속 번호를 돌려주고, 출력은 `term:data`, 끝은 `term:end` 이벤트로 온다.
/// 감사 로그에는 종류, 대상, 사용자 이름만 남긴다. 친 글자와 화면 내용은 남기지 않는다.
#[tauri::command]
pub fn term_open(app: AppHandle, spec: Spec, cols: u16, rows: u16) -> Result<u32, String> {
    let (data, transfer) = dirs(&app)?;
    let id = super::open(spec.clone(), cols, rows, transfer, Arc::new(WindowSink(app)))?;
    crate::audit::log_terminal(&data, spec.kind(), &spec.target(), spec.user());
    Ok(id)
}

#[tauri::command]
pub fn term_write(id: u32, data: String) -> bool {
    data.len() <= 1_048_576 && super::write(id, data.as_bytes())
}

#[tauri::command]
pub fn term_resize(id: u32, cols: u16, rows: u16) -> bool {
    super::resize(id, cols, rows)
}

#[tauri::command]
pub fn term_close(id: u32) -> bool {
    super::close(id)
}

/// 전송 폴더를 파일 탐색기로 열고 경로를 돌려준다.
#[tauri::command]
pub fn term_folder(app: AppHandle) -> Result<String, String> {
    let (_, transfer) = dirs(&app)?;
    let program = if cfg!(target_os = "windows") { "explorer.exe" } else { "xdg-open" };
    let _ = std::process::Command::new(program).arg(&transfer).stdin(std::process::Stdio::null()).spawn();
    Ok(transfer.to_string_lossy().into_owned())
}
