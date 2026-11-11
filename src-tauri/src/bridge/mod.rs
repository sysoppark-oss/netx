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

pub const MODULE_ID: &str = "netx";
pub const API: (u64, u64) = (1, 0);
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
fn target_of(entities: &[Value]) -> Option<String> {
    entities.iter().take(8).find_map(|e| {
        let text = |key: &str| e.get(key).and_then(Value::as_str);
        match text("type")? {
            "endpoint" | "host" | "device" => {
                let ip: IpAddr = text("ip")?.parse().ok()?;
                let port = e.get("port").and_then(Value::as_u64).filter(|p| (1..=65535).contains(p));
                Some(match (ip, port) {
                    (IpAddr::V4(ip), Some(port)) => format!("{ip}:{port}"),
                    (IpAddr::V6(ip), Some(port)) => format!("[{ip}]:{port}"),
                    (ip, None) => ip.to_string(),
                })
            }
            "domain" => {
                let name = text("name")?.to_ascii_lowercase();
                is_domain(&name).then_some(name)
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
        "capabilities": ["widgets", "handoff", "open"],
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
    let target = target_of(entities).ok_or((INVALID_PARAMS, "진단할 수 있는 대상(주소 또는 도메인)이 없습니다"))?;

    // 같은 요청 번호는 한 번만 화면에 알린다.
    let mut seen = SEEN.lock().unwrap_or_else(|e| e.into_inner());
    if seen.iter().any(|s| s == request_id) {
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

    host.handoff(Handoff { target, from: from.to_string(), note: tidy(text("note").unwrap_or(""), 200) });
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
        "rack.handoff" | "rack.open" => Some("operate"),
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
        _ => match handoff(params, host, session) {
            Ok(result) => ok(id, result),
            Err((code, message)) => error(id, code, message),
        },
    }
}

/// 접속자의 실행 파일이 등록부(HKLM, 관리자만 쓸 수 있다)에 올라 있는 랙 구성원인지.
/// 경로는 대소문자와 `/`·`\` 차이를 없애고 통째로 비교한다(폴더가 아니라 파일 하나).
pub fn is_member(image: &str, registered: &[String]) -> bool {
    let norm = |p: &str| p.trim().trim_start_matches(r"\\?\").replace('/', "\\").to_lowercase();
    let image = norm(image);
    !image.is_empty() && registered.iter().any(|r| norm(r) == image)
}

// ---- 화면이 가져갈 인계 ----

static PENDING: Mutex<Option<Handoff>> = Mutex::new(None);

pub fn put_pending(h: Handoff) {
    *PENDING.lock().unwrap_or_else(|e| e.into_inner()) = Some(h);
}

/// 화면이 넘겨받은 대상을 가져간다(한 번만).
#[tauri::command]
pub fn bridge_take_handoff() -> Option<Handoff> {
    PENDING.lock().unwrap_or_else(|e| e.into_inner()).take()
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
        assert_eq!(r["result"], json!({"module":"netx","module_version":"0.1.0","api":[1,0],"granted":["read","operate"]}));
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
        assert_eq!(*host.got.borrow(), [Handoff { target: "203.0.113.9:443".into(), from: "storyfirewall".into(), note: "사건 #4812[31m".into() }]);
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
    }
}
