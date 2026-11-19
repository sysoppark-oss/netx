//! 관제 PC: 한 PC의 NetX가 진단한 결과를 다른 PC의 NetX로 모은다(통합프로젝트 「관제 PC 변경 제안 (2판)」 1단계).
//!
//! NetX의 화면·실행 엔진·검증기를 부르지 않는 독립 모듈이다(나중에 관제로 뗀다). NetX가 넘겨주는 것은
//! 끝난 진단 한 건([`wire::Finished`])뿐이고, Tauri에 닿는 곳은 `app.rs` 하나다.
//!
//! - 통로: 테일스케일 주소(100.64.0.0/10)에만 묶어 듣고, 그 대역에서 온 접속만 받는다. TLS 1.3 상호 인증.
//!   인증 기관 없이, 짝 맺을 때 저장한 공개 키 지문만 받아들인다.
//! - 방향: 보내는 PC가 나가는 연결만 쓴다. 관제 PC는 받은 줄에 답만 한다. 관제 PC가 먼저 보내는 요청은 없다.
//! - 다른 PC로의 조회 의뢰(2단계)의 통로는 여기에 없다.

pub mod app;
pub mod key;
pub mod net;
pub mod store;
pub mod wire;

use std::net::Ipv4Addr;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};
use tokio::sync::{Notify, oneshot};

use key::Identity;
use store::{Inbox, Outbox, Taken};

/// 관제 PC가 듣는 TCP 포트(기본값). IANA의 동적 포트 범위(49152~65535) 안이다.
pub const PORT_DEFAULT: u16 = 49317;
/// 짝 맺기 창이 열려 있는 시간
pub const PAIR_WINDOW: Duration = Duration::from_secs(120);
/// 틀린 코드가 이만큼 쌓이면 그 코드를 버린다.
pub const PAIR_MAX_WRONG: u32 = 5;

/// 테일스케일 대역(100.64.0.0/10)인지
pub fn tailnet(ip: Ipv4Addr) -> bool {
    let o = ip.octets();
    o[0] == 100 && o[1] & 0xc0 == 64
}

pub fn now_ms() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_millis() as u64).unwrap_or(0)
}

fn yes() -> bool {
    true
}

fn keep_default() -> u32 {
    store::KEEP_DAYS_DEFAULT
}

fn port_default() -> u16 {
    PORT_DEFAULT
}

/// 짝 맺은 관제 PC
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Collector {
    /// 공개 키 지문. 이 지문이 아니면 보내지 않는다.
    pub id: String,
    pub name: String,
    /// 테일스케일 주소
    pub addr: String,
    pub port: u16,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Config {
    /// 이 PC의 진단 결과를 관제 PC로 보낸다. 기본 꺼짐 — 이 PC에서 사람이 켠다.
    #[serde(default)]
    pub send: bool,
    /// 보낼 때 내부 주소·PC 이름을 가린다. 기본 켬.
    #[serde(default = "yes")]
    pub mask: bool,
    /// 이 PC를 관제 PC로 쓴다(테일스케일 주소에서 듣는다). 기본 꺼짐.
    #[serde(default)]
    pub receive: bool,
    /// 다른 PC의 화면에 보일 이 PC의 이름
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub collector: Option<Collector>,
    #[serde(default = "keep_default")]
    pub keep_days: u32,
    #[serde(default = "port_default")]
    pub port: u16,
}

impl Default for Config {
    fn default() -> Config {
        Config { send: false, mask: true, receive: false, name: String::new(), collector: None, keep_days: store::KEEP_DAYS_DEFAULT, port: PORT_DEFAULT }
    }
}

const CONFIG_FILE: &str = "config.json";

fn load_config(dir: &Path) -> Config {
    let mut c: Config = std::fs::read_to_string(dir.join(CONFIG_FILE)).ok().and_then(|s| serde_json::from_str(&s).ok()).unwrap_or_default();
    c.keep_days = c.keep_days.clamp(1, store::KEEP_DAYS_MAX);
    if c.port < 1024 {
        c.port = PORT_DEFAULT;
    }
    c.name = wire::text(&c.name, 40);
    // 손으로 고친 설정 파일이라도 테일스케일 밖 주소로는 보내지 않는다.
    c.collector = c.collector.filter(|k| key::is_fingerprint(&k.id) && k.addr.parse::<Ipv4Addr>().is_ok_and(tailnet) && k.port >= 1024);
    c
}

/// 관제 PC 쪽 짝 맺기 창
struct Window {
    code: String,
    until: Instant,
    wrong: u32,
    /// 코드가 맞아 사람의 승인을 기다리는 상대
    pending: Option<Pending>,
}

struct Pending {
    id: String,
    name: String,
    addr: String,
    number: String,
    decide: Option<oneshot::Sender<bool>>,
}

/// 화면에 보이는 짝 맺기 창의 상태
#[derive(Debug, Clone, Serialize, PartialEq, Default)]
pub struct WindowView {
    pub open: bool,
    pub code: String,
    pub secs_left: u64,
    pub wrong: u32,
    pub pending: Option<PendingView>,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct PendingView {
    pub name: String,
    pub addr: String,
    /// 확인 숫자
    pub number: String,
}

/// 보내는 쪽에서 짝 맺기가 어디까지 왔는지
#[derive(Debug, Clone, Serialize, PartialEq, Default)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum Join {
    #[default]
    Idle,
    /// 코드가 맞았다. 두 화면의 확인 숫자를 견주고 관제 PC에서 승인하기를 기다린다.
    Waiting { number: String, name: String, addr: String },
    Done { name: String },
    Failed { why: String },
}

#[derive(Debug, Clone, Serialize, PartialEq, Default)]
pub struct Status {
    /// 듣고 있는 주소(듣지 않으면 없음)
    pub listening: Option<String>,
    /// 듣지 못하는 까닭
    pub listen_error: Option<String>,
    /// 관제 PC에 닿아 있는지
    pub linked: bool,
    pub send_error: Option<String>,
    pub last_sent: u64,
}

pub struct Hub {
    pub dir: PathBuf,
    pub me: Identity,
    pub version: String,
    host: String,
    user: String,
    secret: Vec<u8>,
    config: Mutex<Config>,
    pub inbox: Inbox,
    pub outbox: Outbox,
    window: Mutex<Option<Window>>,
    pub join: Mutex<Join>,
    pub status: Mutex<Status>,
    /// 보낼 것이 생겼거나 설정이 바뀌었다.
    pub wake: Notify,
}

fn lock<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|p| p.into_inner())
}

impl Hub {
    pub fn open(dir: &Path, version: &str, host: &str, user: &str) -> Result<Hub, String> {
        std::fs::create_dir_all(dir).map_err(|e| format!("거점 관제 폴더를 만들지 못했습니다: {e}"))?;
        let me = key::load_or_create(dir)?;
        let mut config = load_config(dir);
        if config.name.is_empty() {
            config.name = wire::text(host, 40);
        }
        let hub = Hub {
            dir: dir.to_path_buf(),
            secret: key::alias_secret(dir)?,
            inbox: Inbox::open(&dir.join("inbox.db"))?,
            outbox: Outbox::open(&dir.join("outbox.db"))?,
            me,
            version: version.to_string(),
            host: host.to_string(),
            user: user.to_string(),
            config: Mutex::new(config),
            window: Mutex::new(None),
            join: Mutex::new(Join::Idle),
            status: Mutex::new(Status::default()),
            wake: Notify::new(),
        };
        if hub.config().receive {
            hub.inbox.pair(&hub.me.id(), &hub.config().name, "", true, now_ms())?;
        }
        Ok(hub)
    }

    pub fn config(&self) -> Config {
        lock(&self.config).clone()
    }

    fn save(&self, config: &Config) -> Result<(), String> {
        let text = serde_json::to_string_pretty(config).map_err(|e| e.to_string())?;
        key::write_private(&self.dir.join(CONFIG_FILE), text.as_bytes())?;
        *lock(&self.config) = config.clone();
        self.wake.notify_waiters();
        Ok(())
    }

    /// 화면에서 바꿀 수 있는 설정을 바꾼다. 짝(collector)은 짝 맺기와 짝 끊기로만 바뀐다.
    pub fn set(&self, send: bool, mask: bool, receive: bool, name: &str, keep_days: u32) -> Result<Config, String> {
        let mut c = self.config();
        let name = wire::text(name, 40);
        if name.is_empty() {
            return Err("이 PC의 이름을 적어 주세요".into());
        }
        if send && c.collector.is_none() {
            return Err("먼저 관제 PC와 짝을 맺어야 보낼 수 있습니다".into());
        }
        if c.send && !send {
            // 끈 뒤에는 기다리던 것도 나가지 않는다.
            self.outbox.clear_queue();
        }
        (c.send, c.mask, c.receive, c.name, c.keep_days) = (send, mask, receive, name, keep_days.clamp(1, store::KEEP_DAYS_MAX));
        if c.receive {
            self.inbox.pair(&self.me.id(), &c.name, "", true, now_ms())?;
        }
        self.save(&c)?;
        Ok(c)
    }

    /// 보내는 쪽: 관제 PC와의 짝을 끊는다. 보내기도 꺼지고 기다리던 것은 비운다.
    pub fn forget_collector(&self) -> Result<(), String> {
        let mut c = self.config();
        c.collector = None;
        c.send = false;
        self.outbox.clear_queue();
        *lock(&self.join) = Join::Idle;
        self.save(&c)
    }

    pub(crate) fn remember_collector(&self, k: Collector) -> Result<(), String> {
        let mut c = self.config();
        c.collector = Some(k);
        self.save(&c)
    }

    /// 끝난 진단 한 건을 받는다. 보내기가 켜져 있으면 가려서 보낼 함에 넣고, 이 PC가 관제 PC가면 자기 관제 PC에도 넣는다(가리지 않고, 네트워크를 거치지 않는다).
    /// 돌려주는 것: 쓴 순번(아무 데도 넣지 않았으면 없음).
    pub fn submit(&self, f: wire::Finished, public_ip: Option<&str>) -> Result<Option<u64>, String> {
        let c = self.config();
        let sending = c.send && c.collector.is_some();
        if !sending && !c.receive {
            return Ok(None);
        }
        let f = wire::tidy(f);
        let me = self.me.id();
        let now = now_ms();
        let mask = wire::Mask::new(c.mask, &self.secret, &self.host, &self.user, public_ip);
        let out = mask.apply(f.clone());
        let Some((seq, _)) = self.outbox.push(&out, sending, now, |seq| wire::encode(&out, &me, seq, c.mask, &self.version))? else {
            return Err("결과가 너무 커서 거점 관제에 넣지 못했습니다".into());
        };
        if c.receive {
            let line = wire::encode(&f, &me, seq, false, &self.version).ok_or("결과가 너무 커서 거점 관제에 넣지 못했습니다")?;
            if let Taken::Refused(why) = self.inbox.take(&me, seq, &line, now)? {
                return Err(format!("이 PC의 거점 관제가 받지 않았습니다: {}", why.code()));
            }
        }
        if sending {
            self.wake.notify_waiters();
        }
        Ok(Some(seq))
    }

    // ---- 관제 PC 쪽 짝 맺기 창 ----

    /// [새 PC 짝 맺기]: 창을 열고 일회용 코드를 띄운다. 이미 열려 있으면 새 코드로 바꾼다.
    pub fn open_window(&self) -> Result<WindowView, String> {
        if !self.config().receive {
            return Err("먼저 \"이 PC를 관제 PC로 쓰기\"를 켜야 합니다".into());
        }
        *lock(&self.window) = Some(Window { code: key::new_code()?, until: Instant::now() + PAIR_WINDOW, wrong: 0, pending: None });
        Ok(self.window_view())
    }

    pub fn close_window(&self) {
        // 기다리던 상대에게는 거절로 전해진다(결정 통로가 닫힌다).
        *lock(&self.window) = None;
    }

    pub fn window_view(&self) -> WindowView {
        let mut w = lock(&self.window);
        if w.as_ref().is_some_and(|w| Instant::now() >= w.until) {
            *w = None;
        }
        match w.as_ref() {
            None => WindowView::default(),
            Some(w) => WindowView {
                open: true,
                code: w.code.clone(),
                secs_left: w.until.saturating_duration_since(Instant::now()).as_secs(),
                wrong: w.wrong,
                pending: w.pending.as_ref().map(|p| PendingView { name: p.name.clone(), addr: p.addr.clone(), number: p.number.clone() }),
            },
        }
    }

    /// 접속해 온 상대가 낸 증명을 본다. 맞으면 (관제 PC의 증명, 확인 숫자, 사람의 결정을 기다리는 통로).
    /// 코드는 한 번만 쓰인다: 맞은 뒤에는 다른 상대가 같은 코드로 들어오지 못한다.
    pub(crate) fn try_pair(&self, sender_id: &str, name: &str, addr: &str, proof: &str) -> Option<(String, String, oneshot::Receiver<bool>)> {
        // 코드에서 키를 만드는 계산이 느리므로 잠금 밖에서 한다.
        let code = {
            let w = lock(&self.window);
            let w = w.as_ref().filter(|w| Instant::now() < w.until && w.pending.is_none())?;
            w.code.clone()
        };
        let mine = self.me.id();
        let k = key::pair_key(&code, sender_id, &mine);
        let good = key::same(&key::proof(&k, "sender"), proof);
        let mut guard = lock(&self.window);
        let w = guard.as_mut().filter(|w| w.code == code && Instant::now() < w.until && w.pending.is_none())?;
        if !good {
            w.wrong += 1;
            if w.wrong >= PAIR_MAX_WRONG {
                *guard = None;
            }
            return None;
        }
        let number = key::confirm_number(sender_id, &mine);
        let (tx, rx) = oneshot::channel();
        w.pending = Some(Pending { id: sender_id.to_string(), name: wire::text(name, 40), addr: addr.to_string(), number: number.clone(), decide: Some(tx) });
        Some((key::proof(&k, "collector"), number, rx))
    }

    /// [짝 맺기 승인] 또는 거절. 승인하면 그 PC의 지문을 저장하고 창을 닫는다.
    pub fn decide(&self, approve: bool) -> Result<(), String> {
        let pending = {
            let mut w = lock(&self.window);
            if w.as_ref().is_some_and(|w| w.pending.is_some()) { w.take().and_then(|w| w.pending) } else { None }
        }
        .ok_or("승인을 기다리는 PC가 없습니다")?;
        if approve {
            self.inbox.pair(&pending.id, &pending.name, &pending.addr, false, now_ms())?;
        }
        if let Some(tx) = pending.decide {
            let _ = tx.send(approve);
        }
        Ok(())
    }

    /// 관제 PC 쪽: 그 PC의 짝을 끊거나(removed), 멈추거나(stopped), 다시 켠다(active).
    pub fn set_pc(&self, id: &str, state: &str) -> Result<(), String> {
        if self.inbox.set_state(id, state)? { Ok(()) } else { Err("그런 PC가 없습니다".into()) }
    }
}

#[cfg(test)]
pub(crate) fn test_hub(name: &str) -> std::sync::Arc<Hub> {
    use std::sync::atomic::{AtomicU32, Ordering};
    static N: AtomicU32 = AtomicU32::new(0);
    let dir = std::env::temp_dir().join(format!("netx-collect-hub-{}-{}-{name}", std::process::id(), N.fetch_add(1, Ordering::Relaxed)));
    let _ = std::fs::remove_dir_all(&dir);
    std::sync::Arc::new(Hub::open(&dir, "test", &format!("{name}-PC"), "kim").unwrap())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn item(target: &str) -> wire::Finished {
        wire::Finished { kind: "route_latency".into(), title: "라우팅 지연".into(), target: target.into(), verdict: "ok".into(), started: now_ms(), ended: now_ms(), ..Default::default() }
    }

    #[test]
    fn tailnet_range_is_exact() {
        for (ip, yes) in [("100.64.0.1", true), ("100.127.255.254", true), ("100.63.255.255", false), ("100.128.0.0", false), ("192.168.0.1", false), ("127.0.0.1", false)] {
            assert_eq!(tailnet(ip.parse().unwrap()), yes, "{ip}");
        }
    }

    #[test]
    fn nothing_leaves_or_is_kept_until_a_person_turns_it_on() {
        let hub = test_hub("off");
        let c = hub.config();
        assert!(!c.send && !c.receive && c.mask, "보내기·받기는 기본 꺼짐, 가리기는 기본 켬");
        assert_eq!(hub.submit(item("192.168.0.9"), None).unwrap(), None);
        assert_eq!(hub.outbox.waiting(), 0);
        assert!(hub.outbox.log().is_empty());
        // 짝이 없으면 보내기를 켤 수 없다
        assert!(hub.set(true, true, false, "용인", 90).is_err());
        assert!(hub.open_window().is_err(), "관제 PC로 쓰지 않는 PC는 짝 맺기 창을 열 수 없다");
    }

    #[test]
    fn own_results_go_to_own_inbox_unmasked_and_sent_copy_is_masked() {
        let hub = test_hub("both");
        hub.set(false, true, true, "평창", 90).unwrap();
        hub.remember_collector(Collector { id: "a".repeat(64), name: "다른 관제 PC".into(), addr: "100.64.0.9".into(), port: PORT_DEFAULT }).unwrap();
        hub.set(true, true, true, "평창", 90).unwrap();
        let seq = hub.submit(item("192.168.0.9"), None).unwrap().unwrap();
        assert_eq!(hub.inbox.item(&hub.me.id(), seq).unwrap().target, "192.168.0.9");
        let (queued_seq, line) = hub.outbox.next().unwrap();
        assert_eq!(queued_seq, seq);
        assert!(!line.contains("192.168.0.9") && line.contains("내부-"));
        assert!(line.contains("\"masked\":true"));
        // 보내기를 끄면 기다리던 것도 나가지 않는다
        hub.set(false, true, true, "평창", 90).unwrap();
        assert!(hub.outbox.next().is_none());
        // 짝을 끊으면 보내기도 꺼진다
        hub.set(true, true, true, "평창", 90).unwrap();
        hub.forget_collector().unwrap();
        assert!(!hub.config().send && hub.config().collector.is_none());
    }

    #[test]
    fn config_file_cannot_point_outside_the_tailnet() {
        let hub = test_hub("cfg");
        for (addr, ok) in [("100.64.0.9", true), ("8.8.8.8", false), ("192.168.0.2", false), ("evil.example", false)] {
            let text = serde_json::json!({"send": true, "collector": {"id": "b".repeat(64), "name": "x", "addr": addr, "port": 49317}}).to_string();
            std::fs::write(hub.dir.join(CONFIG_FILE), text).unwrap();
            assert_eq!(load_config(&hub.dir).collector.is_some(), ok, "{addr}");
        }
        assert!(load_config(&hub.dir).mask, "설정 파일에 없으면 가리기는 켬");
    }

    #[test]
    fn pairing_code_is_single_use_and_dies_after_five_wrong_tries() {
        let hub = test_hub("pair");
        hub.set(false, true, true, "평창", 90).unwrap();
        let sender = "c".repeat(64);
        let proof_for = |code: &str| key::proof(&key::pair_key(code, &sender, &hub.me.id()), "sender");

        // 창이 닫혀 있으면 맞는 증명도 받지 않는다
        assert!(hub.try_pair(&sender, "용인", "100.64.0.2", &proof_for("ANYTHING22")).is_none());

        let code = hub.open_window().unwrap().code;
        for _ in 0..PAIR_MAX_WRONG - 1 {
            assert!(hub.try_pair(&sender, "용인", "100.64.0.2", &proof_for("WRONGWRONG")).is_none());
        }
        assert_eq!(hub.window_view().wrong, PAIR_MAX_WRONG - 1);
        let (reply, number, rx) = hub.try_pair(&sender, "용\u{202E}인", "100.64.0.2", &proof_for(&code)).unwrap();
        assert_eq!(reply, key::proof(&key::pair_key(&code, &sender, &hub.me.id()), "collector"));
        assert_eq!(hub.window_view().pending.unwrap(), PendingView { name: "용인".into(), addr: "100.64.0.2".into(), number });
        // 승인을 기다리는 동안 같은 코드로 또 들어올 수 없다
        assert!(hub.try_pair(&"d".repeat(64), "가짜", "100.64.0.3", &key::proof(&key::pair_key(&code, &"d".repeat(64), &hub.me.id()), "sender")).is_none());
        // 승인 전에는 짝이 아니다
        assert!(hub.inbox.admitted(&sender).is_err());
        hub.decide(true).unwrap();
        assert!(rx.blocking_recv().unwrap());
        assert!(hub.inbox.admitted(&sender).is_ok());
        assert!(!hub.window_view().open, "승인하면 창이 닫힌다");
        assert!(hub.decide(true).is_err());

        // 다섯 번 틀리면 코드를 버린다
        hub.open_window().unwrap();
        for _ in 0..PAIR_MAX_WRONG {
            assert!(hub.try_pair(&sender, "용인", "100.64.0.2", &proof_for("WRONGWRONG")).is_none());
        }
        assert!(!hub.window_view().open);

        // 거절하면 짝이 되지 않는다
        let code = hub.open_window().unwrap().code;
        let other = "e".repeat(64);
        let (_, _, rx) = hub.try_pair(&other, "남", "100.64.0.4", &key::proof(&key::pair_key(&code, &other, &hub.me.id()), "sender")).unwrap();
        hub.decide(false).unwrap();
        assert!(!rx.blocking_recv().unwrap());
        assert!(hub.inbox.admitted(&other).is_err());
    }
}
