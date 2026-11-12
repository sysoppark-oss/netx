//! 입구의 Windows 쪽: 네임드 파이프 서버, 접속자 확인, 등록부.
//!
//! - 파이프 이름은 켤 때마다 무작위다. 첫 인스턴스를 만든 뒤에 `HKCU\SOFTWARE\Antioch\Running\netx`에 적는다.
//! - 원격 접속을 거부한다. 파이프의 접근 권한은 Windows 기본값이다(만든 사용자·관리자·시스템만 쓸 수 있고,
//!   다른 사용자는 읽기만 되어 요청을 보낼 수 없다).
//! - 접속자 확인: 접속한 프로세스의 실행 파일 경로가 `HKLM\SOFTWARE\Antioch\Modules\<id>`의 `Exe`와 같을 때만
//!   권한을 준다. HKLM은 관리자만 쓸 수 있으므로, 관리자가 설치한 랙 구성원만 통과한다.
//!   (스토리방화벽 공개판은 판마다 쓰고 버리는 자체 서명 키를 쓰므로 서명자 이름으로는 확인할 수 없다.)

use std::os::windows::io::AsRawHandle;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

use tauri::{AppHandle, Emitter, Manager};
use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};
use tokio::net::windows::named_pipe::{NamedPipeServer, PipeMode, ServerOptions};
use winreg::enums::{HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE};
use winreg::RegKey;

use super::{Caller, Handoff, Host, Session, MAX_REQUEST_BYTES};
use crate::audit::{self, AuditEntry};
use crate::model::{Level, Risk};

const RUNNING_KEY: &str = r"SOFTWARE\Antioch\Running\netx";
const MODULES_KEY: &str = r"SOFTWARE\Antioch\Modules";
/// 동시에 받는 접속 수
const MAX_CLIENTS: usize = 4;
/// 이만큼 아무 요청이 없으면 접속을 끊는다.
const IDLE: Duration = Duration::from_secs(300);

static CLIENTS: AtomicUsize = AtomicUsize::new(0);
/// 입구 상태: 0 열지 못함(또는 아직), 1 열렸고 스스로 접속해 인사에 답하는 것까지 확인함, 2 열렸지만 확인 실패
static STATE: AtomicUsize = AtomicUsize::new(0);

/// "정보" 화면에 보일 상태: (상태, 등록된 랙 구성원 수)
pub fn status() -> (usize, usize) {
    (STATE.load(Ordering::SeqCst), registered_members().len())
}

/// 자기 입구에 접속해 인사를 보내고 답을 받는다. 통로가 실제로 열려 있는지 확인하는 용도다.
async fn self_check(name: &str) -> bool {
    use tokio::net::windows::named_pipe::ClientOptions;
    let Ok(client) = ClientOptions::new().open(name) else { return false };
    let (reader, mut writer) = tokio::io::split(client);
    let hello = "{\"jsonrpc\":\"2.0\",\"id\":1,\"method\":\"rack.hello\",\"params\":{\"client\":\"netx-selfcheck\",\"api\":[1,0]}}\n";
    if writer.write_all(hello.as_bytes()).await.is_err() {
        return false;
    }
    let mut line = String::new();
    let read = tokio::time::timeout(Duration::from_secs(5), BufReader::new(reader).read_line(&mut line)).await;
    matches!(read, Ok(Ok(n)) if n > 0) && line.contains("\"module\":\"netx\"")
}

struct AppHost(AppHandle);

impl Host for AppHost {
    fn version(&self) -> String {
        self.0.package_info().version.to_string()
    }

    fn full(&self) -> bool {
        crate::env_check::full()
    }

    fn focus(&self) {
        if let Some(window) = self.0.get_webview_window("main") {
            let _ = window.unminimize();
            // Windows가 창을 앞으로 가져오지 못하게 막아도 작업 표시줄 단추는 깜박인다.
            let _ = window.request_user_attention(Some(tauri::UserAttentionType::Informational));
            let _ = window.set_focus();
        }
    }

    fn handoff(&self, h: Handoff) {
        // NetX가 앞에 있지 않으면 Windows 알림 창으로도 알린다.
        let focused = self.0.get_webview_window("main").and_then(|w| w.is_focused().ok()).unwrap_or(false);
        if !focused {
            use tauri_plugin_notification::NotificationExt;
            let (title, body) = super::notice_text(&h);
            let _ = self.0.notification().builder().title(title).body(body).show();
        }
        if let Ok(dir) = self.0.path().app_local_data_dir() {
            // 감사 로그: 누가(보낸 모듈) 무엇을(대상) 넘겼는지. 실행은 아니다.
            audit::log(&dir, &AuditEntry { time: audit::now(), run_id: 0, level: Level::General, source: "bridge", tool: "rack.handoff", argv: &[h.target.clone(), h.from.clone()], risk: Risk::Safe, root: false, confirmed: false });
        }
        super::put_pending(h);
        let _ = self.0.emit("bridge:handoff", ());
    }
}

/// 파이프 이름: 운영체제 난수 128비트. 난수를 얻지 못하면 입구를 열지 않는다.
fn random_name() -> Option<String> {
    let mut bytes = [0u8; 16];
    getrandom::fill(&mut bytes).ok()?;
    Some(format!(r"\\.\pipe\netx-{}", bytes.iter().map(|b| format!("{b:02x}")).collect::<String>()))
}

fn create(name: &str, first: bool) -> std::io::Result<NamedPipeServer> {
    ServerOptions::new().first_pipe_instance(first).reject_remote_clients(true).pipe_mode(PipeMode::Message).max_instances(MAX_CLIENTS + 1).create(name)
}

fn publish(name: &str) {
    let exe = std::env::current_exe().map(|p| p.to_string_lossy().into_owned()).unwrap_or_default();
    if let Ok((key, _)) = RegKey::predef(HKEY_CURRENT_USER).create_subkey(RUNNING_KEY) {
        let _ = key.set_value("PipeName", &name);
        let _ = key.set_value("Exe", &exe);
        let _ = key.set_value("Api", &"1.0");
        let _ = key.set_value("Kind", &"app");
    }
}

/// NetX가 꺼질 때 등록부에서 지운다.
pub fn unpublish() {
    let _ = RegKey::predef(HKEY_CURRENT_USER).delete_subkey(RUNNING_KEY);
}

/// 운영체제가 알려 주는 최종 경로(짧은 이름과 연결을 푼 것). 파일이 없으면 None.
pub(crate) fn final_path(path: &str) -> Option<String> {
    Some(std::fs::canonicalize(path).ok()?.to_string_lossy().into_owned())
}

/// 관리자만 쓸 수 있는 프로그램 폴더들. 환경 변수가 아니라 HKLM에서 읽는다(환경 변수는 실행한 쪽이 바꿀 수 있다).
pub(crate) fn admin_dirs() -> Vec<String> {
    let Ok(key) = RegKey::predef(HKEY_LOCAL_MACHINE).open_subkey(r"SOFTWARE\Microsoft\Windows\CurrentVersion") else { return Vec::new() };
    ["ProgramFilesDir", "ProgramFilesDir (x86)", "ProgramW6432Dir"].iter().filter_map(|name| final_path(&key.get_value::<String, _>(name).ok()?)).collect()
}

/// 관리자가 등록한 랙 구성원의 실행 파일 경로들(모듈마다 `Exe`와, 있으면 `ClientExe`).
/// 관리자 전용 폴더 아래에 실제로 있는 파일만 남긴다.
fn registered_members() -> Vec<String> {
    let Ok(modules) = RegKey::predef(HKEY_LOCAL_MACHINE).open_subkey(MODULES_KEY) else { return Vec::new() };
    let found: Vec<String> = modules
        .enum_keys()
        .flatten()
        .take(32)
        .filter_map(|id| modules.open_subkey(id).ok())
        .flat_map(|key| super::MEMBER_VALUES.iter().filter_map(|name| key.get_value::<String, _>(*name).ok()).collect::<Vec<String>>())
        .filter_map(|raw| final_path(&raw))
        .collect();
    super::admitted(found, &admin_dirs())
}

fn client_pid(server: &NamedPipeServer) -> Option<u32> {
    use windows::Win32::Foundation::HANDLE;
    use windows::Win32::System::Pipes::GetNamedPipeClientProcessId;
    let mut pid = 0u32;
    unsafe { GetNamedPipeClientProcessId(HANDLE(server.as_raw_handle()), &mut pid) }.ok()?;
    (pid != 0).then_some(pid)
}

/// 접속자의 실행 파일 경로. 프로세스 핸들을 먼저 잡고, 그 핸들이 지금도 이 파이프의 접속자이며 살아 있는지
/// 확인한 다음 그 핸들로 경로를 읽는다(번호가 다른 프로세스로 바뀌는 틈을 막는다).
fn client_image(server: &NamedPipeServer) -> Option<(u32, String)> {
    use windows::core::PWSTR;
    use windows::Win32::Foundation::CloseHandle;
    use windows::Win32::System::Threading::{GetExitCodeProcess, OpenProcess, QueryFullProcessImageNameW, PROCESS_NAME_WIN32, PROCESS_QUERY_LIMITED_INFORMATION};
    const STILL_ACTIVE: u32 = 259;
    let pid = client_pid(server)?;
    unsafe {
        let process = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid).ok()?;
        let read = || {
            let mut code = 0u32;
            GetExitCodeProcess(process, &mut code).ok()?;
            if code != STILL_ACTIVE || client_pid(server) != Some(pid) {
                return None;
            }
            let mut buf = vec![0u16; 32_768];
            let mut len = buf.len() as u32;
            QueryFullProcessImageNameW(process, PROCESS_NAME_WIN32, PWSTR(buf.as_mut_ptr()), &mut len).ok()?;
            Some(String::from_utf16_lossy(buf.get(..len as usize)?))
        };
        let image = read();
        let _ = CloseHandle(process);
        Some((pid, image?))
    }
}

fn caller_of(server: &NamedPipeServer) -> Caller {
    let Some((pid, image)) = client_image(server) else { return Caller::default() };
    // 비교는 운영체제의 최종 경로끼리 한다.
    let member = final_path(&image).is_some_and(|image| super::is_member(&image, &registered_members()));
    Caller { pid, image, granted: if member { vec!["read", "operate"] } else { Vec::new() } }
}

async fn serve(server: NamedPipeServer, app: AppHandle) {
    let caller = caller_of(&server);
    let host = AppHost(app);
    let mut session = Session::default();
    let (reader, mut writer) = tokio::io::split(server);
    let mut reader = BufReader::with_capacity(MAX_REQUEST_BYTES + 4096, reader);
    loop {
        let mut line = Vec::new();
        let read = tokio::time::timeout(IDLE, (&mut reader).take(MAX_REQUEST_BYTES as u64 + 2).read_until(b'\n', &mut line)).await;
        match read {
            Ok(Ok(n)) if n > 0 && line.ends_with(b"\n") => {}
            // 끊김, 오류, 너무 긴 줄, 오래 조용함: 접속을 닫는다.
            _ => break,
        }
        let Ok(text) = String::from_utf8(line) else { break };
        let reply = super::handle(&text, &caller, &host, &mut session);
        if writer.write_all(reply.as_bytes()).await.is_err() || writer.flush().await.is_err() {
            break;
        }
    }
}

/// 입구를 연다. NetX가 켜져 있는 동안 돈다.
pub fn start(app: AppHandle) {
    tauri::async_runtime::spawn(async move {
        let Some(name) = random_name() else { return };
        let Ok(mut server) = create(&name, true) else { return };
        publish(&name);
        {
            let name = name.clone();
            tauri::async_runtime::spawn(async move {
                tokio::time::sleep(Duration::from_secs(2)).await;
                STATE.store(if self_check(&name).await { 1 } else { 2 }, Ordering::SeqCst);
            });
        }
        loop {
            if server.connect().await.is_err() {
                tokio::time::sleep(Duration::from_secs(1)).await;
                match create(&name, false) {
                    Ok(next) => server = next,
                    Err(_) => break,
                }
                continue;
            }
            // 다음 접속을 받을 인스턴스를 먼저 만든 뒤 지금 접속을 처리한다.
            let connected = match create(&name, false) {
                Ok(next) => std::mem::replace(&mut server, next),
                Err(_) => break,
            };
            if CLIENTS.load(Ordering::SeqCst) >= MAX_CLIENTS {
                continue; // 받지 않고 닫는다.
            }
            CLIENTS.fetch_add(1, Ordering::SeqCst);
            let app = app.clone();
            tauri::async_runtime::spawn(async move {
                serve(connected, app).await;
                CLIENTS.fetch_sub(1, Ordering::SeqCst);
            });
        }
        STATE.store(0, Ordering::SeqCst);
        unpublish();
    });
}
