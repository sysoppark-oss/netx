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
            let _ = window.set_focus();
        }
    }

    fn handoff(&self, h: Handoff) {
        if let Ok(dir) = self.0.path().app_local_data_dir() {
            // 감사 로그: 누가(보낸 모듈) 무엇을(대상) 넘겼는지. 실행은 아니다.
            audit::log(&dir, &AuditEntry { time: audit::now(), run_id: 0, level: Level::General, source: "bridge", tool: "rack.handoff", argv: &[h.target.clone(), h.from.clone()], risk: Risk::Safe, root: false, confirmed: false });
        }
        super::put_pending(h);
        let _ = self.0.emit("bridge:handoff", ());
    }
}

fn random_name() -> String {
    use std::collections::hash_map::RandomState;
    use std::hash::{BuildHasher, Hasher};
    let part = || {
        let mut h = RandomState::new().build_hasher();
        h.write_u128(std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_nanos()).unwrap_or(0));
        h.write_u32(std::process::id());
        h.finish()
    };
    format!(r"\\.\pipe\netx-{:016x}{:016x}", part(), part())
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

/// 관리자가 등록한 랙 구성원의 실행 파일 경로들
fn registered_members() -> Vec<String> {
    let Ok(modules) = RegKey::predef(HKEY_LOCAL_MACHINE).open_subkey(MODULES_KEY) else { return Vec::new() };
    modules.enum_keys().flatten().take(32).filter_map(|id| modules.open_subkey(id).ok()?.get_value::<String, _>("Exe").ok()).collect()
}

fn client_pid(server: &NamedPipeServer) -> Option<u32> {
    use windows::Win32::Foundation::HANDLE;
    use windows::Win32::System::Pipes::GetNamedPipeClientProcessId;
    let mut pid = 0u32;
    unsafe { GetNamedPipeClientProcessId(HANDLE(server.as_raw_handle()), &mut pid) }.ok()?;
    (pid != 0).then_some(pid)
}

fn image_of(pid: u32) -> Option<String> {
    use windows::core::PWSTR;
    use windows::Win32::Foundation::CloseHandle;
    use windows::Win32::System::Threading::{OpenProcess, QueryFullProcessImageNameW, PROCESS_NAME_WIN32, PROCESS_QUERY_LIMITED_INFORMATION};
    unsafe {
        let process = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid).ok()?;
        let mut buf = vec![0u16; 32_768];
        let mut len = buf.len() as u32;
        let got = QueryFullProcessImageNameW(process, PROCESS_NAME_WIN32, PWSTR(buf.as_mut_ptr()), &mut len);
        let _ = CloseHandle(process);
        got.ok()?;
        Some(String::from_utf16_lossy(buf.get(..len as usize)?))
    }
}

fn caller_of(server: &NamedPipeServer) -> Caller {
    let pid = client_pid(server).unwrap_or(0);
    let image = if pid == 0 { String::new() } else { image_of(pid).unwrap_or_default() };
    let granted = if super::is_member(&image, &registered_members()) { vec!["read", "operate"] } else { Vec::new() };
    Caller { pid, image, granted }
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
        let name = random_name();
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
