//! 넷엑스 터미널: 시리얼, SSH, Telnet, FTP, TFTP 접속을 별도 창의 탭으로 연다.
//!
//! 원칙
//! - 암호를 받는 칸이 없다. 암호는 접속한 뒤 상대의 프롬프트에 직접 치고, NetX는 저장하거나 기록하지 않는다.
//!   SSH는 Windows에 들어 있는 OpenSSH를 가상 터미널 안에서 돌리므로 암호·호스트 키는 ssh가 직접 다룬다.
//! - 주소, 포트, 사용자 이름, 시리얼 포트 이름은 형식을 검사한 값만 쓴다. 셸을 거치지 않는다.
//! - FTP·TFTP 파일은 전송 폴더 안에서만 주고받는다(`files`).
//! - 이 모듈은 Tauri를 모른다. 출력은 `Sink`로 내보낸다(창과의 연결은 `cmd`).

pub mod cmd;
mod console;
mod files;
mod ftp;
mod pty;
mod serial;
mod telnet;
mod tftp;

use std::collections::HashMap;
use std::net::{IpAddr, SocketAddr, TcpStream, ToSocketAddrs};
use std::path::PathBuf;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::mpsc::{Sender, channel};
use std::sync::{Arc, LazyLock, Mutex};
use std::time::Duration;

use serde::Deserialize;

/// 한 번에 열어 둘 수 있는 접속 수
const MAX_SESSIONS: usize = 12;
const CONNECT_TIMEOUT: Duration = Duration::from_secs(10);

/// 화면이 보낸 접속 정보. `check`를 통과한 뒤에만 쓴다.
#[derive(Deserialize, Clone, Debug, PartialEq)]
#[serde(tag = "kind", rename_all = "lowercase")]
pub enum Spec {
    Ssh { host: String, port: u16, user: String },
    Telnet { host: String, port: u16 },
    Serial { port: String, baud: u32, data_bits: u8, parity: String, stop_bits: u8, flow: String },
    Ftp { host: String, port: u16, user: Option<String> },
    Tftp { host: String, port: u16 },
}

pub const BAUDS: [u32; 12] = [300, 1200, 2400, 4800, 9600, 19200, 38400, 57600, 115200, 230400, 460800, 921600];

impl Spec {
    pub fn kind(&self) -> &'static str {
        match self {
            Spec::Ssh { .. } => "ssh",
            Spec::Telnet { .. } => "telnet",
            Spec::Serial { .. } => "serial",
            Spec::Ftp { .. } => "ftp",
            Spec::Tftp { .. } => "tftp",
        }
    }

    /// 감사 로그와 탭 이름에 쓰는 대상 표기
    pub fn target(&self) -> String {
        match self {
            Spec::Ssh { host, port, .. } | Spec::Telnet { host, port } | Spec::Ftp { host, port, .. } | Spec::Tftp { host, port } => format!("{host}:{port}"),
            Spec::Serial { port, baud, .. } => format!("{port} {baud}"),
        }
    }

    pub fn user(&self) -> Option<&str> {
        match self {
            Spec::Ssh { user, .. } => Some(user),
            Spec::Ftp { user, .. } => user.as_deref(),
            _ => None,
        }
    }

    /// 형식 검사. 사용자 입력은 여기서 걸러진 값만 프로그램 인자나 접속 주소가 된다.
    pub fn check(&self) -> Result<(), String> {
        match self {
            Spec::Ssh { host, port, user } => {
                check_host(host)?;
                check_port(*port)?;
                check_user(user)
            }
            Spec::Telnet { host, port } | Spec::Tftp { host, port } => {
                check_host(host)?;
                check_port(*port)
            }
            Spec::Ftp { host, port, user } => {
                check_host(host)?;
                check_port(*port)?;
                match user.as_deref().map(str::trim) {
                    None | Some("") => Ok(()),
                    Some(u) => check_user(u),
                }
            }
            Spec::Serial { port, baud, data_bits, parity, stop_bits, flow } => {
                check_serial_port(port)?;
                if !BAUDS.contains(baud) {
                    return Err("지원하지 않는 속도입니다".into());
                }
                if !matches!(data_bits, 5..=8) {
                    return Err("데이터 비트는 5~8입니다".into());
                }
                if !matches!(parity.as_str(), "none" | "even" | "odd") {
                    return Err("패리티 값이 올바르지 않습니다".into());
                }
                if !matches!(stop_bits, 1 | 2) {
                    return Err("정지 비트는 1 또는 2입니다".into());
                }
                if !matches!(flow.as_str(), "none" | "rtscts" | "xonxoff") {
                    return Err("흐름 제어 값이 올바르지 않습니다".into());
                }
                Ok(())
            }
        }
    }
}

/// IP 주소이거나, 영문·숫자·'.'·'-'로만 된 호스트 이름
pub fn check_host(host: &str) -> Result<(), String> {
    if host.parse::<IpAddr>().is_ok() {
        return Ok(());
    }
    let ok = !host.is_empty()
        && host.len() <= 253
        && host.chars().all(|c| c.is_ascii_alphanumeric() || c == '.' || c == '-')
        && host.split('.').all(|label| !label.is_empty() && label.len() <= 63 && !label.starts_with('-') && !label.ends_with('-'));
    if ok { Ok(()) } else { Err("주소는 IP 주소이거나 영문·숫자·'.'·'-'로 된 호스트 이름이어야 합니다".into()) }
}

fn check_port(port: u16) -> Result<(), String> {
    if port == 0 { Err("포트 번호가 올바르지 않습니다".into()) } else { Ok(()) }
}

pub fn check_user(user: &str) -> Result<(), String> {
    if user.is_empty() {
        return Err("사용자 이름을 입력하세요".into());
    }
    if user.len() > 32 || user.starts_with('-') || !user.chars().all(|c| c.is_ascii_alphanumeric() || "._-".contains(c)) {
        return Err("사용자 이름에는 영문, 숫자, '.', '_', '-'만 쓸 수 있습니다".into());
    }
    Ok(())
}

/// Windows는 `COM<숫자>`, 그 밖에는 `/dev/tty<영문·숫자>`만 받는다.
pub fn check_serial_port(port: &str) -> Result<(), String> {
    let ok = if cfg!(target_os = "windows") {
        port.strip_prefix("COM").is_some_and(|n| !n.is_empty() && n.len() <= 3 && n.chars().all(|c| c.is_ascii_digit()))
    } else {
        port.strip_prefix("/dev/tty").is_some_and(|n| !n.is_empty() && n.len() <= 24 && n.chars().all(|c| c.is_ascii_alphanumeric()))
    };
    if ok { Ok(()) } else { Err("시리얼 포트 이름이 올바르지 않습니다".into()) }
}

/// 접속으로 들어가는 것: 사용자가 친 글자, 창 크기, 닫기, 그리고 읽는 쪽 스레드가 알리는 끝.
pub enum Input {
    Data(Vec<u8>),
    Resize(u16, u16),
    Close,
    Eof(String),
}

/// 접속에서 나오는 것을 받는 쪽(창).
pub trait Sink: Send + Sync + 'static {
    fn data(&self, id: u32, bytes: &[u8]);
    fn end(&self, id: u32, message: &str);
}

/// 한 접속의 출력 통로
#[derive(Clone)]
pub struct Out {
    id: u32,
    sink: Arc<dyn Sink>,
}

impl Out {
    /// 상대가 보낸 글자를 그대로 내보낸다.
    pub fn bytes(&self, bytes: &[u8]) {
        if !bytes.is_empty() {
            self.sink.data(self.id, bytes);
        }
    }

    /// NetX가 쓰는 글(줄바꿈을 터미널 줄바꿈으로 바꾼다).
    pub fn text(&self, text: &str) {
        self.bytes(text.replace('\n', "\r\n").as_bytes());
    }

    /// NetX의 안내 줄. 상대가 보낸 글과 구별되게 흐리게 보인다.
    pub fn note(&self, text: &str) {
        self.bytes(format!("\x1b[2m{}\x1b[0m\r\n", text.replace('\n', "\r\n")).as_bytes());
    }
}

/// 상대가 보낸 글에서 터미널 제어 문자를 지운다(FTP 응답·목록, TFTP 오류 글처럼 글자로만 보여야 하는 것).
pub fn plain(text: &str) -> String {
    text.chars().filter(|c| *c != '\r').map(|c| if c == '\n' || c == '\t' || !c.is_control() { c } else { '?' }).collect()
}

static SESSIONS: LazyLock<Mutex<HashMap<u32, Sender<Input>>>> = LazyLock::new(|| Mutex::new(HashMap::new()));
static NEXT_ID: AtomicU32 = AtomicU32::new(1);

fn sessions<T>(f: impl FnOnce(&mut HashMap<u32, Sender<Input>>) -> T) -> T {
    f(&mut SESSIONS.lock().unwrap_or_else(|e| e.into_inner()))
}

/// 열려 있는 접속이 있는지 (자동 업데이트가 설치를 미룰지 정할 때 쓴다)
pub fn busy() -> bool {
    sessions(|m| !m.is_empty())
}

/// 접속을 연다. 바로 번호를 돌려주고, 실제 접속은 따로 도는 스레드가 한다.
/// `dir`은 FTP·TFTP가 파일을 주고받는 전송 폴더다.
pub fn open(spec: Spec, cols: u16, rows: u16, dir: PathBuf, sink: Arc<dyn Sink>) -> Result<u32, String> {
    spec.check()?;
    let (tx, rx) = channel();
    let id = sessions(|m| {
        if m.len() >= MAX_SESSIONS {
            return Err(format!("접속은 {MAX_SESSIONS}개까지 열 수 있습니다"));
        }
        let id = NEXT_ID.fetch_add(1, Ordering::Relaxed);
        m.insert(id, tx.clone());
        Ok(id)
    })?;
    let out = Out { id, sink: sink.clone() };
    let size = (cols.clamp(20, 500), rows.clamp(5, 300));
    let spawned = std::thread::Builder::new().name(format!("term-{id}")).spawn(move || {
        let message = match spec {
            Spec::Ssh { host, port, user } => pty::run(&host, port, &user, size, tx, rx, out),
            Spec::Telnet { host, port } => telnet::run(&host, port, size, tx, rx, out),
            Spec::Serial { port, baud, data_bits, parity, stop_bits, flow } => serial::run(&port, baud, data_bits, &parity, stop_bits, &flow, tx, rx, out),
            Spec::Ftp { host, port, user } => ftp::run(&host, port, user.as_deref(), &dir, rx, out),
            Spec::Tftp { host, port } => tftp::run(&host, port, &dir, rx, out),
        };
        sessions(|m| m.remove(&id));
        sink.end(id, &message);
    });
    if spawned.is_err() {
        sessions(|m| m.remove(&id));
        return Err("접속을 시작하지 못했습니다".into());
    }
    Ok(id)
}

fn send(id: u32, input: Input) -> bool {
    sessions(|m| m.get(&id).map(|tx| tx.send(input).is_ok()).unwrap_or(false))
}

pub fn write(id: u32, data: &[u8]) -> bool {
    send(id, Input::Data(data.to_vec()))
}

pub fn resize(id: u32, cols: u16, rows: u16) -> bool {
    send(id, Input::Resize(cols.clamp(20, 500), rows.clamp(5, 300)))
}

pub fn close(id: u32) -> bool {
    send(id, Input::Close)
}

/// 터미널 창이 닫힐 때 모든 접속을 닫는다.
pub fn close_all() {
    sessions(|m| {
        for tx in m.values() {
            let _ = tx.send(Input::Close);
        }
    });
}

/// 이름을 주소로 바꿔 TCP로 접속한다(주소마다 10초).
fn connect(host: &str, port: u16) -> Result<TcpStream, String> {
    let mut last = "주소를 찾지 못했습니다".to_string();
    for addr in resolve(host, port)? {
        match TcpStream::connect_timeout(&addr, CONNECT_TIMEOUT) {
            Ok(stream) => return Ok(stream),
            Err(e) => last = e.to_string(),
        }
    }
    Err(format!("{host}:{port}에 접속하지 못했습니다: {last}"))
}

fn resolve(host: &str, port: u16) -> Result<Vec<SocketAddr>, String> {
    let addrs: Vec<SocketAddr> = (host, port).to_socket_addrs().map_err(|e| format!("{host}의 주소를 찾지 못했습니다: {e}"))?.collect();
    if addrs.is_empty() { Err(format!("{host}의 주소를 찾지 못했습니다")) } else { Ok(addrs) }
}

/// 시험에서 쓰는 가짜 창
#[cfg(test)]
pub(crate) mod testing {
    use super::*;
    use std::sync::mpsc::Receiver;

    #[derive(Default)]
    pub struct Seen(Mutex<Vec<u8>>);

    impl Seen {
        pub fn text(&self) -> String {
            String::from_utf8_lossy(&self.0.lock().unwrap()).into_owned()
        }
    }

    impl Sink for Seen {
        fn data(&self, _id: u32, bytes: &[u8]) {
            self.0.lock().unwrap().extend_from_slice(bytes);
        }
        fn end(&self, _id: u32, _message: &str) {}
    }

    pub fn session() -> (Out, Arc<Seen>, Sender<Input>, Receiver<Input>) {
        let seen = Arc::new(Seen::default());
        let (tx, rx) = channel();
        (Out { id: 1, sink: seen.clone() }, seen, tx, rx)
    }

    pub fn temp_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("netx-{name}-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn host_must_be_ip_or_plain_name() {
        for ok in ["192.168.123.21", "::1", "fe80::1", "switch-1.lan", "a", "sw1"] {
            assert!(check_host(ok).is_ok(), "{ok}");
        }
        for bad in ["", "-oProxyCommand=x", "a b", "a;b", "a..b", ".a", "a-.b", "host/path", "user@host", "a\nb", "$(x)", "한글"] {
            assert!(check_host(bad).is_err(), "{bad}");
        }
        assert!(check_host(&"a".repeat(64)).is_err());
    }

    #[test]
    fn user_and_port() {
        assert!(check_user("admin").is_ok());
        assert!(check_user("net_ops-1.a").is_ok());
        for bad in ["", "-l", "a b", "root;id", "a@b", &"a".repeat(33)] {
            assert!(check_user(bad).is_err(), "{bad}");
        }
        assert!(Spec::Telnet { host: "10.0.0.1".into(), port: 0 }.check().is_err());
        assert!(Spec::Ftp { host: "10.0.0.1".into(), port: 21, user: None }.check().is_ok());
        assert!(Spec::Ftp { host: "10.0.0.1".into(), port: 21, user: Some("a b".into()) }.check().is_err());
    }

    #[test]
    fn serial_settings() {
        let port = if cfg!(target_os = "windows") { "COM3" } else { "/dev/ttyUSB0" };
        let spec = |baud, data_bits, parity: &str, stop_bits, flow: &str| Spec::Serial { port: port.into(), baud, data_bits, parity: parity.into(), stop_bits, flow: flow.into() };
        assert!(spec(9600, 8, "none", 1, "none").check().is_ok());
        assert!(spec(115200, 7, "even", 2, "rtscts").check().is_ok());
        assert!(spec(1234, 8, "none", 1, "none").check().is_err());
        assert!(spec(9600, 9, "none", 1, "none").check().is_err());
        assert!(spec(9600, 8, "mark", 1, "none").check().is_err());
        assert!(spec(9600, 8, "none", 3, "none").check().is_err());
        assert!(spec(9600, 8, "none", 1, "dtr").check().is_err());
        for bad in ["", "COM", "../etc/passwd", "/dev/sda", "/dev/tty/../sda", "COM1 ", "\\\\.\\pipe\\x"] {
            assert!(check_serial_port(bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn spec_reads_screen_json() {
        let spec: Spec = serde_json::from_str(r#"{"kind":"ssh","host":"10.0.0.1","port":22,"user":"admin"}"#).unwrap();
        assert_eq!(spec, Spec::Ssh { host: "10.0.0.1".into(), port: 22, user: "admin".into() });
        assert_eq!((spec.kind(), spec.target().as_str(), spec.user()), ("ssh", "10.0.0.1:22", Some("admin")));
        let spec: Spec = serde_json::from_str(r#"{"kind":"serial","port":"COM3","baud":9600,"data_bits":8,"parity":"none","stop_bits":1,"flow":"none"}"#).unwrap();
        assert_eq!(spec.target(), "COM3 9600");
        assert!(serde_json::from_str::<Spec>(r#"{"kind":"rsh","host":"a","port":1}"#).is_err());
    }

    #[test]
    fn plain_removes_terminal_control() {
        assert_eq!(plain("a\x1b[31mb\r\nc\x07\td"), "a?[31mb\nc?\td");
    }
}
