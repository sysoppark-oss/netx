//! 다른 프로그램(스토리방화벽, 통합 화면 "안디옥")이 NetX에 닿는 입구.
//!
//! 통신 규격 "에피스틀(Epistle) 1.0"의 공통 메서드(`rack.*`) 가운데 일부만 받는다. 근거 문서는
//! `docs/RACK_PLATFORM_DRAFT.md`(0.12판 초안)의 5장, 6장, 8.2장, 8.3장이다. 초안이므로 규격이 바뀌면 여기도 바뀐다.
//!
//! 지키는 선:
//! - 이 입구로는 아무것도 실행되지 않는다. 넘겨받은 대상은 입력칸에 채워질 뿐이고 실행은 사람이 누른다.
//! - 넘겨받은 값은 자료다. 형식을 검사한 값만 화면으로 보낸다(주소는 다시 만들어 쓴다).
//! - 접속자가 스스로 권한을 주장하는 칸은 없다. 권한은 NetX가 접속자를 보고 정한다(`Caller::granted`).
//! - 한 줄 = 한 요청(JSON-RPC 2.0), 1MB 상한. 모르는 메서드는 "없는 메서드"로 답한다.

use std::collections::VecDeque;
use std::net::IpAddr;
use std::sync::Mutex;

use serde::Serialize;
use serde_json::{json, Value};

#[cfg(target_os = "windows")]
pub mod pipe;
pub mod result;

pub const MODULE_ID: &str = "netx";
pub const API: (u64, u64) = (1, 1);
pub const MAX_REQUEST_BYTES: usize = 1024 * 1024;

// 오류 코드 (초안 6.9, 스토리방화벽 현행과 같다)
const PARSE_ERROR: i64 = -32700;
const INVALID_REQUEST: i64 = -32600;
const METHOD_NOT_FOUND: i64 = -32601;
const INVALID_PARAMS: i64 = -32602;
const UNAUTHORIZED: i64 = -32001;
const VERSION_MISMATCH: i64 = -32006;
const UNSUPPORTED_INTENT: i64 = -32010;
const RATE_LIMITED: i64 = -32012;

/// 접속자. 전송 계층이 채운다. 접속자가 보낸 값이 아니다.
#[derive(Debug, Clone, Default)]
pub struct Caller {
    pub pid: u32,
    pub image: String,
    /// "read", "operate"
    pub granted: Vec<&'static str>,
}

impl Caller {
    fn may(&self, grant: &str) -> bool {
        self.granted.iter().any(|g| *g == grant)
    }
}

/// 넘겨받은 진단 대상. 화면이 입력칸에 채운다.
#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct Handoff {
    /// 입력칸에 넣을 글: 주소, 주소:포트, 도메인 이름 가운데 하나
    pub target: String,
    /// 보낸 모듈 id
    pub from: String,
    /// 보낸 쪽이 붙인 한 줄 (제어 문자를 지운 글자)
    pub note: String,
    /// 의뢰 번호. 이 대상으로 한 진단의 결과를 보낸 쪽에 돌려줄 때 쓴다.
    pub request_id: String,
    /// 그 주소에 붙은 이름(꼬리표). 화면에 글자로 보이기만 한다. 조회하거나 명령에 쓰지 않는다.
    pub name: Option<String>,
}

/// Windows 알림 창에 쓸 제목과 본문. 보낸 곳과 숫자 주소만 싣는다(이름·덧붙인 글은 싣지 않는다).
pub fn notice_text(h: &Handoff) -> (String, String) {
    let who = if h.from == "storyfirewall" { "스토리방화벽" } else { h.from.as_str() };
    ("NetX — 진단 대상을 넘겨받았습니다".to_string(), format!("{who}에서 보낸 대상: {}\nNetX 창에서 확인하세요. 자동으로 실행하지 않습니다.", h.target))
}

/// 입구가 NetX 본체에 묻거나 시키는 것. 시험에서는 가짜를 쓴다.
pub trait Host {
    fn version(&self) -> String;
    /// 전체 진단(WSL)을 쓸 수 있는지
    fn full(&self) -> bool;
    /// 창을 앞으로 가져온다.
    fn focus(&self);
    /// 넘겨받은 대상을 화면에 알린다.
    fn handoff(&self, h: Handoff);
}

/// 접속 하나의 상태: 잦은 인계를 막는 데 쓴다.
#[derive(Default)]
pub struct Session {
    last_handoff: Option<std::time::Instant>,
    /// 이 접속이 결과를 기다리는 의뢰 번호들
    waiting: Vec<String>,
}

impl Session {
    /// 그 의뢰의 결과가 나오면 이 접속으로 받는다.
    fn wait_for(&mut self, request_id: &str) {
        if self.waiting.len() < 16 && !self.waiting.iter().any(|w| w == request_id) && result::join(request_id) {
            self.waiting.push(request_id.to_string());
        }
    }
}

impl Drop for Session {
    fn drop(&mut self) {
        result::leave(&self.waiting);
    }
}

/// 이미 처리한 인계 요청 번호(멱등 키). 같은 번호가 다시 오면 화면에 또 알리지 않는다.
static SEEN: Mutex<VecDeque<String>> = Mutex::new(VecDeque::new());

fn error(id: Value, code: i64, message: &str) -> String {
    let mut line = json!({"jsonrpc": "2.0", "id": id, "error": {"code": code, "message": message}}).to_string();
    line.push('\n');
    line
}

fn ok(id: Value, result: Value) -> String {
    let mut line = json!({"jsonrpc": "2.0", "id": id, "result": result}).to_string();
    line.push('\n');
    line
}

/// 밖에서 온 글자: 제어 문자를 지우고 길이를 줄인다.
fn tidy(text: &str, max: usize) -> String {
    text.chars().filter(|c| !c.is_control()).take(max).collect::<String>().trim().to_string()
}

fn is_module_id(v: &str) -> bool {
    !v.is_empty() && v.len() <= 32 && v.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-' || c == '_')
}

fn is_uuid(v: &str) -> bool {
    let parts: Vec<&str> = v.split('-').collect();
    parts.len() == 5 && parts.iter().zip([8, 4, 4, 4, 12]).all(|(p, n)| p.len() == n && p.chars().all(|c| c.is_ascii_hexdigit()))
}

fn is_domain(v: &str) -> bool {
    v.len() <= 253
        && v.contains('.')
        && v.split('.').all(|label| {
            !label.is_empty() && label.len() <= 63 && !label.starts_with('-') && !label.ends_with('-') && label.chars().all(|c| c.is_ascii_alphanumeric() || c == '-')
        })
        && v.parse::<IpAddr>().is_err()
}

/// 공통 대상(`entities`)에서 NetX가 진단할 수 있는 첫 대상을 고른다. 값은 형식을 검사하고 다시 만들어 쓴다.
/// 주소에 이름이 함께 오면(`name`) 형식이 맞는 것만 꼬리표로 돌려준다. 대상은 언제나 숫자 주소다.
fn target_of(entities: &[Value]) -> Option<(String, Option<String>)> {
    entities.iter().take(8).find_map(|e| {
        let text = |key: &str| e.get(key).and_then(Value::as_str);
        match text("type")? {
            "endpoint" | "host" | "device" => {
                let ip: IpAddr = text("ip")?.parse().ok()?;
                let port = e.get("port").and_then(Value::as_u64).filter(|p| (1..=65535).contains(p));
                let target = match (ip, port) {
                    (IpAddr::V4(ip), Some(port)) => format!("{ip}:{port}"),
                    (IpAddr::V6(ip), Some(port)) => format!("[{ip}]:{port}"),
                    (ip, None) => ip.to_string(),
                };
                let label = text("name").map(str::to_ascii_lowercase).filter(|name| is_domain(name));
                Some((target, label))
            }
            "domain" => {
                let name = text("name")?.to_ascii_lowercase();
                is_domain(&name).then_some((name, None))
            }
            _ => None,
        }
    })
}

fn manifest(host: &dyn Host) -> Value {
    json!({
        "id": MODULE_ID,
        "title": "NetX",
        "kind": "app",
        "version": host.version(),
        "api": [API.0, API.1],
        "summary": "상황을 적으면 네트워크 진단 도구와 옵션을 골라 주고 결과를 풀이한다",
        "capabilities": ["widgets", "handoff", "open", "results"],
        "namespaces": [],
        "widgets": [
            {"id": "diagnosis", "title": "진단 준비", "kind": "status", "sizes": ["1x1"], "refresh_sec": 60}
        ],
        "intents_in": ["diagnose.target"],
        "intents_out": ["firewall.block.request"],
        "actions": []
    })
}

fn health(host: &dyn Host) -> Value {
    let full = host.full();
    json!({
        "state": "ok",
        "since": 0,
        "notes": if full { json!([]) } else { json!(["기본 진단으로 동작 중입니다(WSL 없음). 패킷 캡처와 대역폭 측정은 쓸 수 없습니다."]) },
        "checks": [{"id": "wsl", "state": if full { "ok" } else { "degraded" }}]
    })
}

fn widgets(host: &dyn Host, params: &Value) -> Value {
    let wanted: Option<Vec<&str>> = params.get("ids").and_then(Value::as_array).map(|ids| ids.iter().filter_map(Value::as_str).collect());
    let mut list = Vec::new();
    if wanted.as_ref().map(|w| w.contains(&"diagnosis")).unwrap_or(true) {
        let full = host.full();
        list.push(json!({
            "id": "diagnosis",
            "kind": "status",
            "state": "ok",
            "value": if full { "전체 진단" } else { "기본 진단" },
            "open": {"view": "home"}
        }));
    }
    json!({"widgets": list})
}

fn handoff(params: &Value, host: &dyn Host, session: &mut Session) -> Result<Value, (i64, &'static str)> {
    let text = |key: &str| params.get(key).and_then(Value::as_str);
    let request_id = text("request_id").filter(|v| is_uuid(v)).ok_or((INVALID_PARAMS, "request_id는 UUID여야 합니다"))?;
    let from = text("from").filter(|v| is_module_id(v)).ok_or((INVALID_PARAMS, "from이 올바르지 않습니다"))?;
    if text("intent") != Some("diagnose.target") {
        return Err((UNSUPPORTED_INTENT, "받지 않는 인텐트입니다"));
    }
    let entities = params.get("entities").and_then(Value::as_array).ok_or((INVALID_PARAMS, "entities가 없습니다"))?;
    let (target, name) = target_of(entities).ok_or((INVALID_PARAMS, "진단할 수 있는 대상(주소 또는 도메인)이 없습니다"))?;

    // 같은 요청 번호는 한 번만 화면에 알린다.
    let mut seen = SEEN.lock().unwrap_or_else(|e| e.into_inner());
    if seen.iter().any(|s| s == request_id) {
        drop(seen);
        session.wait_for(request_id);
        return Ok(json!({"accepted": true, "opened": true}));
    }
    // 한 접속이 1초에 한 번보다 자주 인계하면 거절한다(창을 계속 앞으로 끌어오는 것을 막는다).
    let now = std::time::Instant::now();
    if session.last_handoff.is_some_and(|t| now.duration_since(t).as_millis() < 1000) {
        return Err((RATE_LIMITED, "요청이 너무 잦습니다"));
    }
    session.last_handoff = Some(now);
    seen.push_back(request_id.to_string());
    if seen.len() > 64 {
        seen.pop_front();
    }
    drop(seen);

    // 이 대상으로 한 진단이 끝나면 결과를 이 접속으로 돌려준다.
    result::open(request_id, from, &target);
    session.wait_for(request_id);
    host.handoff(Handoff { target, from: from.to_string(), note: tidy(text("note").unwrap_or(""), 200), request_id: request_id.to_string(), name });
    host.focus();
    Ok(json!({"accepted": true, "opened": true}))
}

/// 요청 한 줄을 처리해 응답 한 줄을 만든다.
pub fn handle(line: &str, caller: &Caller, host: &dyn Host, session: &mut Session) -> String {
    let line = line.trim_end_matches(['\r', '\n']);
    if line.len() > MAX_REQUEST_BYTES {
        return error(Value::Null, INVALID_REQUEST, "요청이 너무 큽니다");
    }
    let Ok(req) = serde_json::from_str::<Value>(line) else {
        return error(Value::Null, PARSE_ERROR, "JSON이 아닙니다");
    };
    let id = match req.get("id") {
        Some(id) if id.is_i64() || id.is_string() => id.clone(),
        _ => return error(Value::Null, INVALID_REQUEST, "id가 없습니다"),
    };
    let method = req.get("method").and_then(Value::as_str).unwrap_or("");
    if req.get("jsonrpc").and_then(Value::as_str) != Some("2.0") || method.is_empty() || !method.chars().all(|c| c.is_ascii_alphanumeric() || c == '.' || c == '_') {
        return error(id, INVALID_REQUEST, "요청 형식이 올바르지 않습니다");
    }
    let null = Value::Null;
    let params = req.get("params").unwrap_or(&null);
    if !(params.is_object() || params.is_null()) {
        return error(id, INVALID_PARAMS, "params는 객체여야 합니다");
    }

    // 인사는 누구나 할 수 있다. 그 밖은 권한이 있어야 한다.
    let need = match method {
        "rack.hello" => None,
        "rack.describe" | "rack.health" | "rack.widgets" => Some("read"),
        "rack.handoff" | "rack.open" | "rack.result.get" => Some("operate"),
        _ => return error(id, METHOD_NOT_FOUND, "없는 메서드입니다"),
    };
    if need.is_some_and(|g| !caller.may(g)) {
        return error(id, UNAUTHORIZED, "권한이 없습니다");
    }

    match method {
        "rack.hello" => {
            let major = params.get("api").and_then(|a| a.get(0)).and_then(Value::as_u64);
            let minor = params.get("api").and_then(|a| a.get(1)).and_then(Value::as_u64).unwrap_or(0);
            match major {
                Some(m) if m == API.0 && minor <= API.1 => {
                    ok(id, json!({"module": MODULE_ID, "module_version": host.version(), "api": [API.0, API.1], "granted": caller.granted}))
                }
                Some(_) => error(id, VERSION_MISMATCH, "규격 판이 맞지 않습니다"),
                None => error(id, INVALID_PARAMS, "api가 없습니다"),
            }
        }
        "rack.describe" => ok(id, manifest(host)),
        "rack.health" => ok(id, health(host)),
        "rack.widgets" => ok(id, widgets(host, params)),
        "rack.open" => {
            host.focus();
            ok(id, json!({"opened": true}))
        }
        "rack.result.get" => {
            // 의뢰 번호는 보낸 쪽만 아는 무작위 값이다. 아는 쪽에만 그 의뢰의 결과를 준다.
            match params.get("request_id").and_then(Value::as_str).filter(|v| is_uuid(v)).and_then(|v| result::get(v).map(|found| (v, found))) {
                Some((request_id, found)) => {
                    session.wait_for(request_id);
                    ok(id, found)
                }
                None => error(id, INVALID_PARAMS, "모르는 의뢰 번호입니다"),
            }
        }
        _ => match handoff(params, host, session) {
            Ok(result) => ok(id, result),
            Err((code, message)) => error(id, code, message),
        },
    }
}

/// 경로 비교용: `\\?\` 접두사를 떼고 `/`를 `\`로, 소문자로. 끝의 `\`는 뗀다.
fn norm_path(p: &str) -> String {
    p.trim().trim_start_matches(r"\\?\").replace('/', "\\").trim_end_matches('\\').to_lowercase()
}

/// 경로가 드라이브 글자로 시작하는 전체 경로이고 `..`·`.` 같은 조각이 없는지.
fn plain_absolute(p: &str) -> bool {
    let mut parts = p.split('\\');
    let drive = parts.next().unwrap_or("");
    let drive_ok = drive.len() == 2 && drive.as_bytes().first().is_some_and(u8::is_ascii_alphabetic) && drive.ends_with(':');
    let mut count = 0;
    drive_ok && parts.all(|part| {
        count += 1;
        !part.is_empty() && part != "." && part != ".."
    }) && count >= 1
}

/// 등록된 경로가 관리자만 쓸 수 있는 폴더(Program Files 등) 아래의 파일인지 (설계 8.3의 4번).
/// 사용자 폴더의 실행 파일은 같은 사용자 권한의 프로그램이 바꿔치기할 수 있으므로 등록돼 있어도 받지 않는다.
pub fn under_admin_dirs(exe: &str, admin_dirs: &[String]) -> bool {
    let exe = norm_path(exe);
    plain_absolute(&exe)
        && admin_dirs.iter().any(|dir| {
            let dir = norm_path(dir);
            plain_absolute(&dir) && exe.strip_prefix(&dir).is_some_and(|rest| rest.starts_with('\\') && rest.len() > 1)
        })
}

/// 등록부에서 구성원 경로로 읽는 값 이름 (설계 8.3의 1번). `Exe`는 파이프를 여는 실행 파일,
/// `ClientExe`는 남에게 접속하는 실행 파일이 따로 있을 때 그 경로다(스토리방화벽: 서비스와 화면).
pub const MEMBER_VALUES: [&str; 2] = ["Exe", "ClientExe"];

/// 등록부에서 읽은 경로들 가운데 구성원으로 인정할 것. 값 이름이 무엇이든 조건은 같다:
/// 관리자 전용 폴더 아래에 있어야 한다(설계 8.3의 4번).
pub fn admitted(paths: Vec<String>, admin_dirs: &[String]) -> Vec<String> {
    paths.into_iter().filter(|path| under_admin_dirs(path, admin_dirs)).collect()
}

/// 접속자의 실행 파일이 등록부(HKLM, 관리자만 쓸 수 있다)에 올라 있는 랙 구성원인지 (설계 8.3의 2번, 5번).
/// 대소문자와 `\\?\` 접두사 차이를 없애고 전체 경로를 통째로 비교한다(폴더가 아니라 파일 하나).
/// 넘겨받는 두 경로는 운영체제가 알려 준 최종 경로여야 한다(짧은 이름·연결을 푼 것).
pub fn is_member(image: &str, registered: &[String]) -> bool {
    let image = norm_path(image);
    plain_absolute(&image) && registered.iter().any(|r| norm_path(r) == image)
}

// ---- 화면이 가져갈 인계 ----

static PENDING: Mutex<Option<Handoff>> = Mutex::new(None);

pub fn put_pending(h: Handoff) {
    *PENDING.lock().unwrap_or_else(|e| e.into_inner()) = Some(h);
}

/// 접속 하나를 끝까지 받는다: 요청 한 줄에 응답 한 줄, 그리고 이 접속이 기다리는 의뢰의 결과가 나오면 알림 한 줄.
/// `idle`만큼 조용하면 끊는다. 결과를 기다리는 접속은 `idle_waiting`까지 둔다.
pub async fn serve<R, W>(reader: R, mut writer: W, caller: Caller, host: &(dyn Host + Sync), idle: std::time::Duration, idle_waiting: std::time::Duration)
where
    R: tokio::io::AsyncRead + Unpin,
    W: tokio::io::AsyncWrite + Unpin,
{
    use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt};
    let mut session = Session::default();
    let mut feed = result::subscribe();
    let mut reader = tokio::io::BufReader::with_capacity(MAX_REQUEST_BYTES + 4096, reader);
    // 읽다 만 줄은 다음 차례에 이어 읽는다(알림을 보내느라 읽기가 끊겨도 잃지 않는다).
    let mut line: Vec<u8> = Vec::new();
    loop {
        let wait = if session.waiting.is_empty() { idle } else { idle_waiting };
        let room = (MAX_REQUEST_BYTES + 2).saturating_sub(line.len()) as u64;
        let mut limited = (&mut reader).take(room);
        let out = tokio::select! {
            read = tokio::time::timeout(wait, limited.read_until(b'\n', &mut line)) => {
                match read {
                    Ok(Ok(n)) if n > 0 && line.ends_with(b"\n") => {}
                    // 끊김, 오류, 너무 긴 줄, 오래 조용함: 접속을 닫는다.
                    _ => break,
                }
                let Ok(text) = String::from_utf8(std::mem::take(&mut line)) else { break };
                handle(&text, &caller, host, &mut session)
            }
            got = feed.recv() => match got {
                Ok((request_id, note)) if session.waiting.iter().any(|w| *w == request_id) => note,
                Ok(_) | Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => continue,
                Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
            },
        };
        if writer.write_all(out.as_bytes()).await.is_err() || writer.flush().await.is_err() {
            break;
        }
    }
}

/// 화면이 끝난 진단의 기록을 넘기면, 그 의뢰를 보낸 쪽에 결과로 돌려준다.
/// 기록은 [최근 진단]에 남기는 것과 같은 모양이다. 모르는 의뢰 번호면 아무것도 하지 않는다.
#[tauri::command]
pub fn bridge_report(app: tauri::AppHandle, request_id: String, record: Value) -> Option<result::Sent> {
    use tauri::Manager;
    if !is_uuid(&request_id) {
        return None;
    }
    let sent = result::publish(&request_id, &record, crate::audit::now())?;
    if let Ok(dir) = app.path().app_local_data_dir() {
        // 감사 로그: 어느 의뢰의 결과를 누구에게 몇 바이트 보냈는지. 내용은 남기지 않는다.
        let argv = [sent.target.clone(), sent.to.clone(), format!("{}바이트", sent.bytes), if sent.live { "보냄" } else { "보관" }.to_string()];
        crate::audit::log(&dir, &crate::audit::AuditEntry { time: crate::audit::now(), run_id: 0, level: crate::model::Level::General, source: "bridge", tool: "rack.result", argv: &argv, risk: crate::model::Risk::Safe, root: false, confirmed: false });
    }
    Some(sent)
}

/// 화면이 넘겨받은 대상을 가져간다(한 번만).
#[tauri::command]
pub fn bridge_take_handoff() -> Option<Handoff> {
    PENDING.lock().unwrap_or_else(|e| e.into_inner()).take()
}

/// "정보" 화면용: 입구 상태와 등록된 랙 구성원 수. Windows가 아니면 입구가 없다.
#[derive(Debug, Clone, Serialize)]
pub struct BridgeStatus {
    /// "open", "unchecked", "closed", "none"
    pub state: &'static str,
    pub members: usize,
}

#[tauri::command]
pub fn bridge_status() -> BridgeStatus {
    #[cfg(target_os = "windows")]
    {
        let (state, members) = pipe::status();
        BridgeStatus { state: ["closed", "open", "unchecked"].get(state).copied().unwrap_or("closed"), members }
    }
    #[cfg(not(target_os = "windows"))]
    BridgeStatus { state: "none", members: 0 }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;

    #[derive(Default)]
    struct Fake {
        full: bool,
        focused: RefCell<u32>,
        got: RefCell<Vec<Handoff>>,
    }

    impl Host for Fake {
        fn version(&self) -> String {
            "0.1.0".into()
        }
        fn full(&self) -> bool {
            self.full
        }
        fn focus(&self) {
            *self.focused.borrow_mut() += 1;
        }
        fn handoff(&self, h: Handoff) {
            self.got.borrow_mut().push(h);
        }
    }

    fn member() -> Caller {
        Caller { pid: 1, image: "x".into(), granted: vec!["read", "operate"] }
    }

    fn call(line: &str, caller: &Caller, host: &Fake) -> Value {
        let out = handle(line, caller, host, &mut Session::default());
        assert!(out.ends_with('\n') && !out.trim_end().contains('\n'), "응답은 한 줄");
        serde_json::from_str(&out).unwrap()
    }

    fn uuid(n: u32) -> String {
        format!("2f0c1d3e-0000-4000-8000-{n:012x}")
    }

    fn handoff_line(id: &str, entities: Value) -> String {
        json!({"jsonrpc":"2.0","id":7,"method":"rack.handoff","params":{"request_id":id,"intent":"diagnose.target","from":"storyfirewall","entities":entities,"note":"사건 #4812\u{1b}[31m"}}).to_string()
    }

    #[test]
    fn hello_reports_grants_and_checks_version() {
        let host = Fake::default();
        let r = call(r#"{"jsonrpc":"2.0","id":1,"method":"rack.hello","params":{"client":"antioch","client_version":"0.1.0","api":[1,0]}}"#, &member(), &host);
        assert_eq!(r["result"], json!({"module":"netx","module_version":"0.1.0","api":[1,1],"granted":["read","operate"]}));
        // 모르는 접속자도 인사는 되지만 권한은 비어 있다.
        let r = call(r#"{"jsonrpc":"2.0","id":"a","method":"rack.hello","params":{"api":[1,0],"granted":["operate"]}}"#, &Caller::default(), &host);
        assert_eq!((r["id"].clone(), r["result"]["granted"].clone()), (json!("a"), json!([])));
        let r = call(r#"{"jsonrpc":"2.0","id":1,"method":"rack.hello","params":{"api":[2,0]}}"#, &member(), &host);
        assert_eq!(r["error"]["code"], -32006);
        let r = call(r#"{"jsonrpc":"2.0","id":1,"method":"rack.hello","params":{"api":[1,3]}}"#, &member(), &host);
        assert_eq!(r["error"]["code"], -32006, "우리보다 새 마이너는 받지 않는다");
    }

    #[test]
    fn unknown_caller_gets_nothing_else() {
        let host = Fake::default();
        for method in ["rack.describe", "rack.health", "rack.widgets", "rack.open"] {
            let r = call(&json!({"jsonrpc":"2.0","id":1,"method":method}).to_string(), &Caller::default(), &host);
            assert_eq!(r["error"]["code"], -32001, "{method}");
        }
        let r = call(&handoff_line(&uuid(900), json!([{"type":"endpoint","ip":"203.0.113.9"}])), &Caller::default(), &host);
        assert_eq!(r["error"]["code"], -32001);
        // 읽기만 받은 접속자는 인계하지 못한다.
        let reader = Caller { granted: vec!["read"], ..Caller::default() };
        let r = call(&handoff_line(&uuid(901), json!([{"type":"endpoint","ip":"203.0.113.9"}])), &reader, &host);
        assert_eq!(r["error"]["code"], -32001);
        assert!(host.got.borrow().is_empty() && *host.focused.borrow() == 0);
    }

    #[test]
    fn describe_health_widgets() {
        let host = Fake::default();
        let d = call(r#"{"jsonrpc":"2.0","id":1,"method":"rack.describe"}"#, &member(), &host);
        assert_eq!((d["result"]["id"].clone(), d["result"]["kind"].clone()), (json!("netx"), json!("app")));
        assert_eq!(d["result"]["intents_in"], json!(["diagnose.target"]));
        let h = call(r#"{"jsonrpc":"2.0","id":1,"method":"rack.health"}"#, &member(), &host);
        assert_eq!((h["result"]["state"].clone(), h["result"]["checks"][0]["state"].clone()), (json!("ok"), json!("degraded")));
        let w = call(r#"{"jsonrpc":"2.0","id":1,"method":"rack.widgets","params":{"ids":["diagnosis","nope"]}}"#, &member(), &Fake { full: true, ..Fake::default() });
        assert_eq!(w["result"]["widgets"].as_array().unwrap().len(), 1);
        assert_eq!(w["result"]["widgets"][0]["value"], "전체 진단");
    }

    #[test]
    fn handoff_fills_target_once_and_never_runs_anything() {
        let host = Fake::default();
        let r = call(&handoff_line(&uuid(1), json!([{"type":"process","pid":4321,"image":"C:\\x.exe"},{"type":"endpoint","ip":"203.0.113.9","port":443,"proto":"tcp"}])), &member(), &host);
        assert_eq!(r["result"], json!({"accepted": true, "opened": true}));
        assert_eq!(*host.got.borrow(), [Handoff { target: "203.0.113.9:443".into(), from: "storyfirewall".into(), note: "사건 #4812[31m".into(), request_id: uuid(1), name: None }]);
        assert_eq!(*host.focused.borrow(), 1);
        // 같은 요청 번호가 다시 와도 화면에는 한 번만 간다.
        call(&handoff_line(&uuid(1), json!([{"type":"endpoint","ip":"203.0.113.9","port":443}])), &member(), &host);
        assert_eq!(host.got.borrow().len(), 1);

        let pick = |n: u32, entities: Value| {
            let host = Fake::default();
            let r = call(&handoff_line(&uuid(n), entities), &member(), &host);
            let got = host.got.borrow().first().map(|h| h.target.clone());
            (r["error"]["code"].as_i64(), got)
        };
        assert_eq!(pick(2, json!([{"type":"domain","name":"Example.COM"}])), (None, Some("example.com".into())));
        assert_eq!(pick(3, json!([{"type":"endpoint","ip":"2001:db8::1","port":53}])), (None, Some("[2001:db8::1]:53".into())));
        assert_eq!(pick(4, json!([{"type":"host","ip":"10.0.0.5","port":0}])), (None, Some("10.0.0.5".into())));
        // 명령처럼 보이는 값, 형식이 틀린 값은 받지 않는다.
        for (n, bad) in [
            json!([{"type":"endpoint","ip":"10.0.0.5; rm -rf /"}]),
            json!([{"type":"domain","name":"a.com --flag"}]),
            json!([{"type":"domain","name":"-x.com"}]),
            json!([{"type":"file","path":"C:\\x"}]),
            json!([]),
        ]
        .into_iter()
        .enumerate()
        {
            assert_eq!(pick(10 + n as u32, bad.clone()), (Some(-32602), None), "{bad}");
        }
    }

    /// 여러 스레드에서 쓸 수 있는 가짜 본체 (접속 처리 시험용)
    #[derive(Default)]
    struct Quiet(std::sync::Mutex<Vec<Handoff>>);

    impl Host for Quiet {
        fn version(&self) -> String {
            "0.1.0".into()
        }
        fn full(&self) -> bool {
            true
        }
        fn focus(&self) {}
        fn handoff(&self, h: Handoff) {
            self.0.lock().unwrap().push(h);
        }
    }

    #[test]
    fn result_get_answers_only_for_known_requests() {
        let host = Fake::default();
        let id = uuid(120);
        let ask = |id: &str| json!({"jsonrpc":"2.0","id":9,"method":"rack.result.get","params":{"request_id":id}}).to_string();
        assert_eq!(call(&ask(&id), &member(), &host)["error"]["code"], INVALID_PARAMS, "의뢰하지 않은 번호");
        assert_eq!(call(&ask(&id), &Caller::default(), &host)["error"]["code"], UNAUTHORIZED);
        assert_eq!(call(&ask("x"), &member(), &host)["error"]["code"], INVALID_PARAMS);
        call(&handoff_line(&id, json!([{"type":"endpoint","ip":"10.0.0.5","port":80}])), &member(), &host);
        assert_eq!(host.got.borrow()[0].request_id, id);
        let pending = call(&ask(&id), &member(), &host);
        assert_eq!(pending["result"]["state"], "pending");
        assert_eq!(pending["result"]["target"], "10.0.0.5:80");
        result::publish(&id, &json!({"meta": {"light": "green"}}), 5).unwrap();
        let done = call(&ask(&id), &member(), &host);
        assert_eq!((done["result"]["state"].as_str(), done["result"]["verdict"].as_str(), done["result"]["seq"].as_u64()), (Some("done"), Some("ok"), Some(1)));
    }

    #[tokio::test]
    async fn a_waiting_connection_gets_the_result_pushed() {
        use tokio::io::{AsyncBufReadExt, AsyncWriteExt};
        let host: &'static Quiet = Box::leak(Box::default());
        let (near, far) = tokio::io::duplex(1 << 20);
        let (far_read, far_write) = tokio::io::split(far);
        let server = tokio::spawn(serve(far_read, far_write, member(), host, std::time::Duration::from_secs(5), std::time::Duration::from_secs(5)));
        let (near_read, mut near_write) = tokio::io::split(near);
        let mut lines = tokio::io::BufReader::new(near_read).lines();

        let id = uuid(130);
        near_write.write_all((handoff_line(&id, json!([{"type":"endpoint","ip":"10.0.0.7","port":443}])) + "\n").as_bytes()).await.unwrap();
        let reply: Value = serde_json::from_str(&lines.next_line().await.unwrap().unwrap()).unwrap();
        assert_eq!(reply["result"]["accepted"], true);
        assert_eq!(host.0.lock().unwrap()[0].target, "10.0.0.7:443");

        // 다른 의뢰의 결과는 이 접속으로 오지 않는다.
        let other = uuid(131);
        result::open(&other, "storyfirewall", "10.0.0.8");
        assert!(!result::publish(&other, &json!({}), 1).unwrap().live);
        // 이 접속이 의뢰한 것의 결과는 온다.
        let sent = result::publish(&id, &json!({"meta": {"light": "red"}}), 2).unwrap();
        assert!(sent.live);
        let note: Value = serde_json::from_str(&lines.next_line().await.unwrap().unwrap()).unwrap();
        assert_eq!(note["method"], "rack.result");
        assert_eq!((note["params"]["request_id"].as_str(), note["params"]["verdict"].as_str()), (Some(id.as_str()), Some("danger")));

        // 그 뒤에도 요청은 그대로 받는다. 접속이 끊기면 기다리는 접속 수에서 빠진다.
        near_write.write_all(b"{\"jsonrpc\":\"2.0\",\"id\":3,\"method\":\"rack.health\"}\n").await.unwrap();
        let health: Value = serde_json::from_str(&lines.next_line().await.unwrap().unwrap()).unwrap();
        assert_eq!(health["id"], 3);
        drop(near_write);
        drop(lines);
        server.await.unwrap();
        assert!(!result::publish(&id, &json!({}), 3).unwrap().live);
    }

    #[test]
    fn the_notice_names_the_sender_and_the_address_only() {
        let h = Handoff { target: "142.250.197.142:443".into(), from: "storyfirewall".into(), note: "사건 #1".into(), request_id: String::new(), name: Some("edge.microsoft.com".into()) };
        let (title, body) = notice_text(&h);
        assert!(title.contains("넘겨받았습니다"));
        assert!(body.contains("스토리방화벽") && body.contains("142.250.197.142:443") && body.contains("자동으로 실행하지 않습니다"));
        assert!(!body.contains("edge.microsoft.com") && !body.contains("사건"), "이름과 덧붙인 글은 알림 창에 싣지 않는다");
        let other = Handoff { from: "watch".into(), ..h };
        assert!(notice_text(&other).1.starts_with("watch에서"));
    }

    #[test]
    fn a_name_rides_along_as_a_label_and_is_never_the_target() {
        let label = |n: u32, entities: Value| {
            let host = Fake::default();
            call(&handoff_line(&uuid(n), entities), &member(), &host);
            host.got.borrow().first().map(|h| (h.target.clone(), h.name.clone()))
        };
        // 대상은 숫자 주소, 이름은 꼬리표.
        assert_eq!(
            label(71, json!([{"type":"endpoint","ip":"142.250.197.142","port":443,"name":"Edge.Microsoft.COM"}])),
            Some(("142.250.197.142:443".into(), Some("edge.microsoft.com".into())))
        );
        // 이름이 없으면 지금까지와 같다.
        assert_eq!(label(72, json!([{"type":"endpoint","ip":"10.0.0.5","port":80}])), Some(("10.0.0.5:80".into(), None)));
        // 이름 모양이 아니면 이름만 버리고 주소는 받는다.
        for (n, bad) in ["a.com; rm -rf /", "<b>x</b>.com", "-x.com", "nodot", "한글.com", "a..com", ""].into_iter().enumerate() {
            assert_eq!(label(73 + n as u32, json!([{"type":"endpoint","ip":"10.0.0.5","name":bad}])), Some(("10.0.0.5".into(), None)), "{bad}");
        }
        assert_eq!(label(81, json!([{"type":"endpoint","ip":"10.0.0.5","name":["a.com"]}])), Some(("10.0.0.5".into(), None)));
        assert_eq!(label(82, json!([{"type":"endpoint","ip":"10.0.0.5","name":"a".repeat(250) + ".com"}])), Some(("10.0.0.5".into(), None)));
        // 이름만 있고 주소가 없는 endpoint는 받지 않는다.
        assert_eq!(label(83, json!([{"type":"endpoint","name":"edge.microsoft.com"}])), None);
    }

    #[test]
    fn handoff_rejects_bad_requests() {
        let host = Fake::default();
        let line = |params: Value| json!({"jsonrpc":"2.0","id":1,"method":"rack.handoff","params":params}).to_string();
        let e = json!([{"type":"endpoint","ip":"10.0.0.5"}]);
        let code = |params: Value| call(&line(params), &member(), &host)["error"]["code"].as_i64();
        assert_eq!(code(json!({"request_id":"x","intent":"diagnose.target","from":"storyfirewall","entities":e})), Some(-32602));
        assert_eq!(code(json!({"request_id":uuid(50),"intent":"diagnose.target","from":"Story Firewall!","entities":e})), Some(-32602));
        assert_eq!(code(json!({"request_id":uuid(51),"intent":"firewall.block.request","from":"antioch","entities":e})), Some(-32010));
        assert!(host.got.borrow().is_empty());

        // 한 접속에서 잇달아 오는 인계는 거절한다.
        let mut session = Session::default();
        let first = handle(&handoff_line(&uuid(60), e.clone()), &member(), &host, &mut session);
        let second = handle(&handoff_line(&uuid(61), e.clone()), &member(), &host, &mut session);
        assert!(first.contains("accepted") && second.contains("-32012"), "{first} {second}");
    }

    #[test]
    fn envelope_errors_and_garbage() {
        let host = Fake::default();
        let code = |line: &str| call(line, &member(), &host)["error"]["code"].as_i64();
        assert_eq!(code("not json"), Some(-32700));
        assert_eq!(code(r#"{"jsonrpc":"2.0","method":"rack.health"}"#), Some(-32600));
        assert_eq!(code(r#"{"jsonrpc":"1.0","id":1,"method":"rack.health"}"#), Some(-32600));
        assert_eq!(code(r#"{"jsonrpc":"2.0","id":1,"method":"rack health"}"#), Some(-32600));
        assert_eq!(code(r#"{"jsonrpc":"2.0","id":1,"method":"rack.health","params":[1]}"#), Some(-32602));
        assert_eq!(code(r#"{"jsonrpc":"2.0","id":1,"method":"connections.list"}"#), Some(-32601));
        assert_eq!(code(r#"{"jsonrpc":"2.0","id":1,"method":"rack.subscribe"}"#), Some(-32601));
        let big = format!(r#"{{"jsonrpc":"2.0","id":1,"method":"rack.health","params":{{"x":"{}"}}}}"#, "a".repeat(MAX_REQUEST_BYTES));
        assert_eq!(code(&big), Some(-32600));
        // 무작위 입력에도 죽지 않고 한 줄로 답한다.
        let mut seed = 0x9e37_79b9_7f4a_7c15u64;
        for _ in 0..2000 {
            seed ^= seed << 13;
            seed ^= seed >> 7;
            seed ^= seed << 17;
            let bytes: Vec<u8> = (0..(seed % 200) as usize).map(|i| (seed >> (i % 56)) as u8).collect();
            let out = handle(&String::from_utf8_lossy(&bytes), &member(), &host, &mut Session::default());
            assert!(out.ends_with('\n'));
        }
    }

    #[test]
    fn membership_is_exact_path() {
        let reg = vec![r"C:\Program Files\Story Firewall\story-firewall-ui.exe".to_string()];
        assert!(is_member(r"c:\program files\story firewall\STORY-FIREWALL-UI.EXE", &reg));
        assert!(is_member(r"\\?\C:\Program Files\Story Firewall\story-firewall-ui.exe", &reg));
        assert!(!is_member(r"C:\Users\me\story-firewall-ui.exe", &reg));
        assert!(!is_member(r"C:\Program Files\Story Firewall\story-firewall-ui.exe.evil.exe", &reg));
        assert!(!is_member(r"C:\Program Files\Story Firewall", &reg));
        assert!(!is_member("", &reg) && !is_member("x", &[]) && !is_member("", &[String::new()]));
        assert!(!is_member(r"C:\Program Files\Story Firewall\..\Story Firewall\story-firewall-ui.exe", &reg), "돌아가는 경로는 받지 않는다");
    }

    #[test]
    fn client_exe_counts_as_a_member_on_the_same_terms() {
        assert_eq!(MEMBER_VALUES, ["Exe", "ClientExe"]);
        let dirs = vec![r"C:\Program Files".to_string()];
        // 스토리방화벽: Exe는 서비스, ClientExe는 화면. NetX에 접속해 오는 것은 화면이다.
        let service = r"C:\Program Files\Story Firewall\story-firewall-svc.exe".to_string();
        let screen = r"C:\Program Files\Story Firewall\story-firewall-ui.exe".to_string();
        let members = admitted(vec![service.clone(), screen.clone()], &dirs);
        assert_eq!(members, [service.clone(), screen.clone()]);
        assert!(is_member(r"\\?\c:\program files\story firewall\STORY-FIREWALL-UI.EXE", &members), "화면(ClientExe)도 구성원이다");
        assert!(is_member(&service, &members));
        assert!(!is_member(r"C:\Program Files\Story Firewall\other.exe", &members), "같은 폴더의 다른 파일은 아니다");

        // ClientExe가 사용자 폴더를 가리키면 등록돼 있어도 인정하지 않는다. Exe는 그대로 인정한다.
        let loose = r"C:\Users\me\AppData\Local\Story Firewall\story-firewall-ui.exe".to_string();
        let members = admitted(vec![service.clone(), loose.clone()], &dirs);
        assert_eq!(members, [service.clone()]);
        assert!(!is_member(&loose, &members));
        for bad in [r"C:\Program Files\..\Users\me\ui.exe", r"\\server\share\ui.exe", "ui.exe", ""] {
            assert!(admitted(vec![bad.to_string()], &dirs).is_empty(), "{bad}");
        }
        assert!(admitted(vec![screen], &[]).is_empty(), "관리자 전용 폴더를 모르면 아무도 인정하지 않는다");
    }

    #[test]
    fn registered_path_must_sit_under_an_admin_only_folder() {
        let dirs = vec![r"C:\Program Files".to_string(), r"C:\Program Files (x86)\".to_string()];
        assert!(under_admin_dirs(r"C:\Program Files\Story Firewall\story-firewall-ui.exe", &dirs));
        assert!(under_admin_dirs(r"\\?\c:\program files (x86)\NetX\netx.exe", &dirs));
        for bad in [
            r"C:\Users\me\AppData\Local\NetX\netx.exe",
            r"C:\Program Files Evil\netx.exe",
            r"C:\Program Files",
            r"C:\Program Files\",
            r"C:\Program Files\..\Users\me\evil.exe",
            r"C:\Program Files\.\x.exe",
            r"C:\Program Files\\x.exe",
            r"\\server\share\Program Files\x.exe",
            r"Program Files\x.exe",
            r"D:\Program Files\x.exe",
            "",
        ] {
            assert!(!under_admin_dirs(bad, &dirs), "{bad}");
        }
        assert!(!under_admin_dirs(r"C:\Program Files\x.exe", &[]));
        assert!(!under_admin_dirs(r"C:\x.exe", &[r"C:".to_string()]), "드라이브 뿌리는 관리자 전용 폴더가 아니다");
        assert!(!under_admin_dirs(r"C:\Program Files\x.exe", &[r"..\Program Files".to_string(), String::new()]));
    }
}
